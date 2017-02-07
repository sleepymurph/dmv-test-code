use constants;
use dag;
use encodable;
use error::*;
use rustc_serialize;
use rustc_serialize::json;
use std::collections;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hash;
use std::hash::Hasher;
use std::io;
use std::io::Read;
use std::io::Write;
use std::ops;
use std::path;


/// Status of a file's cached hash
#[derive(Clone,Eq,PartialEq,Debug)]
pub enum CacheStatus {
    /// File's hash is not cached
    NotCached { size: dag::ObjectSize },
    /// File's hash is cached, but it has been modified since
    Modified { size: dag::ObjectSize },
    /// File's hash is cached
    Cached { hash: dag::ObjectKey },
}

/// Does an early return with the cached hash value if it is present
///
/// Like a `try!` for caching.
#[macro_export]
macro_rules! return_if_cached {
    (do_check; $path:expr, $cache_check:expr) => {
        if let Ok($crate::cache::CacheStatus::Cached{ hash }) = $cache_check {
                debug!("Already hashed: {} {}", hash, $path.display());
                return Ok(hash);
        }
    };
    ($cache:expr, $path:expr) => {
        return_if_cached!{do_check; $path, $cache.check($path)};
    };
    ($cache:expr, $path:expr, $metadata:expr) => {
        return_if_cached!{do_check; $path, $cache.check_with($path, $metadata)};
    };
}

/// Does an early return if the cached value matches
///
/// Like a `try!` for caching.
#[macro_export]
macro_rules! return_if_cache_matches {
    (do_check; $path:expr, $hash:expr, $cache_check:expr) => {
        if $path.exists() {
            match $cache_check {
                Ok($crate::cache::CacheStatus::Cached { hash: ref cache_hash })
                    if cache_hash == $hash => {
                        debug!("Already at state: {} {}",
                                cache_hash, $path.display());
                        return Ok(());
                }
                _ => {}
            }
        }
    };
    ($cache:expr, $path:expr, $hash:expr) => {
        return_if_cache_matches!{do_check; $path, $hash,
                                    $cache.check($path)};
    };
    ($cache:expr, $path:expr, $metadata:expr, $hash:expr) => {
        return_if_cache_matches!{do_check; $path, $hash,
                                    $cache.check_with($path, $metadata)};
    };
}

type CacheMap = collections::HashMap<encodable::PathBuf, CacheEntry>;

wrapper_struct!{
/// A cache of known file hashes
#[derive(Clone,Eq,PartialEq,Debug)]
pub struct HashCache(CacheMap);
}

/// Data stored in the cache for each file
#[derive(Clone,Hash,Eq,PartialEq,Debug,RustcEncodable,RustcDecodable)]
pub struct CacheEntry {
    pub filestats: FileStats,
    pub hash: dag::ObjectKey,
}

/// Subset of file metadata used to determine if file has been modified
#[derive(Clone,Hash,Eq,PartialEq,Debug,RustcEncodable,RustcDecodable)]
pub struct FileStats {
    size: dag::ObjectSize,
    mtime: encodable::SystemTime,
}

/// A file-backed cache that saves updates on drop
pub struct HashCacheFile {
    /// Path to the file that backs this cache
    cache_file_path: path::PathBuf,
    /// The cache map itself
    cache: HashCache,
    /// A hash of the cache's state on disk, to prevent unnecessary writes
    on_disk_state: u64,
}

/// Cache of caches
pub struct AllCaches {
    // TODO: Use an actual cache that can purge entries
    directory_caches: collections::HashMap<path::PathBuf, HashCacheFile>,
}

// HashCache

impl HashCache {
    pub fn new() -> Self { HashCache(CacheMap::new()) }

    pub fn insert_entry(&mut self,
                        file_path: path::PathBuf,
                        file_stats: FileStats,
                        hash: dag::ObjectKey) {

        debug!("Caching file hash: {} => {}", file_path.display(), hash);
        self.0.insert(file_path.into(),
                      CacheEntry {
                          filestats: file_stats,
                          hash: hash,
                      });
    }

    pub fn get<'a, P: ?Sized + AsRef<path::Path>>(&self,
                                                  file_path: &'a P)
                                                  -> Option<&CacheEntry> {
        self.0.get(&file_path.into())
    }

    pub fn check<'a, P: ?Sized + AsRef<path::Path>>(&self,
                                                    file_path: &'a P,
                                                    file_stats: &FileStats)
                                                    -> CacheStatus {
        match self.0.get(&file_path.into()) {
            Some(cache_entry) => {
                if cache_entry.filestats == *file_stats {
                    CacheStatus::Cached { hash: cache_entry.hash }
                } else {
                    CacheStatus::Modified { size: file_stats.size }
                }
            }
            None => CacheStatus::NotCached { size: file_stats.size },
        }
    }

    fn calculate_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl rustc_serialize::Encodable for HashCache {
    fn encode<S: rustc_serialize::Encoder>
        (&self,
         s: &mut S)
         -> ::std::result::Result<(), S::Error> {
        rustc_serialize::Encodable::encode(&self.0, s)
    }
}

impl rustc_serialize::Decodable for HashCache {
    fn decode<D: rustc_serialize::Decoder>
        (d: &mut D)
         -> ::std::result::Result<Self, D::Error> {
        let cache_map =
            try!(<CacheMap as rustc_serialize::Decodable>::decode(d));
        Ok(HashCache(cache_map))
    }
}

impl Hash for HashCache {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for entry in &self.0 {
            entry.hash(state);
        }
    }
}

// FileStats

impl FileStats {
    pub fn read(file_path: &path::Path) -> io::Result<Self> {
        fs::metadata(file_path).map(|x| x.into())
    }
}

impl From<fs::Metadata> for FileStats {
    fn from(metadata: fs::Metadata) -> FileStats {
        FileStats {
            size: metadata.len(),
            mtime: metadata.modified()
                .expect("system has no mod time in file stats")
                .into(),
        }
    }
}

// HashCacheFile

impl_deref_mut!(HashCacheFile => HashCache, cache);

impl HashCacheFile {
    /// Create/open a cache file at a specific location
    pub fn open(cache_file_path: path::PathBuf) -> Result<Self> {
        let cache_file_exists = cache_file_path.exists();

        let cache_map = if cache_file_exists {
            debug!("Opening cache: {} (existing)", cache_file_path.display());
            let mut cache_file = try!(fs::OpenOptions::new()
                .read(true)
                .open(&cache_file_path));

            let mut json_str = String::new();
            try!(cache_file.read_to_string(&mut json_str));
            try!(json::decode(&json_str).map_err(|e| {
                ErrorKind::CorruptCacheFile {
                    cache_file: cache_file_path.to_owned(),
                    cause: e,
                    bad_json: json_str,
                }
            }))
        } else {
            debug!("Opening cache: {} (new)", cache_file_path.display());
            CacheMap::new()
        };
        let cache_map = HashCache(cache_map);

        Ok(HashCacheFile {
            cache_file_path: cache_file_path,
            on_disk_state: cache_map.calculate_hash(),
            cache: cache_map,
        })
    }

    /// Create/open a cache file in the given directory
    ///
    /// The file will be named according to `constants::CACHE_FILE_NAME`.
    pub fn open_in_dir(dir_path: &path::Path) -> Result<Self> {
        Self::open(dir_path.join(constants::CACHE_FILE_NAME))
    }

    /// Create/open a cache file in the parent directory of the given file
    ///
    /// For this app, cache files for files in a directory are stored in that
    /// directory. This is a convenience method to find/create the cache file
    /// responsible for the given file.
    pub fn open_in_parent_dir(child_path: &path::Path) -> Result<Self> {
        let dir_path = try!(child_path.parent_or_err());
        Self::open_in_dir(dir_path)
    }

    pub fn flush(&mut self) -> Result<()> {
        let cur_state = self.cache.calculate_hash();
        if cur_state == self.on_disk_state {
            debug!("Cache unchanged: {}", self.cache_file_path.display());
            return Ok(());
        }
        debug!("Writing cache: {}", self.cache_file_path.display());
        let mut cache_file = try!(fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.cache_file_path));
        try!(write!(cache_file, "{}", json::as_pretty_json(&self.cache.0)));
        self.on_disk_state = cur_state;
        Ok(())
    }
}

impl ops::Drop for HashCacheFile {
    fn drop(&mut self) { self.flush().expect("Could not flush hash file") }
}

// AllCaches

impl AllCaches {
    pub fn new() -> Self {
        AllCaches { directory_caches: collections::HashMap::new() }
    }

    fn cache_for_dir(&mut self,
                     dir_path: &path::Path)
                     -> Result<&mut HashCacheFile> {
        if self.directory_caches.get(dir_path).is_none() {
            let cache_file = try!(HashCacheFile::open_in_dir(dir_path));
            self.directory_caches.insert(dir_path.into(), cache_file);
        }
        Ok(self.directory_caches.get_mut(dir_path).expect("just inserted"))
    }

    pub fn check(&mut self, file_path: &path::Path) -> Result<CacheStatus> {
        let metadata = try!(file_path.metadata());
        self.check_with(file_path, &metadata.into())
    }

    pub fn check_with(&mut self,
                      file_path: &path::Path,
                      stats: &FileStats)
                      -> Result<CacheStatus> {

        let dir_path = try!(file_path.parent_or_err());
        let dir_cache = try!(self.cache_for_dir(dir_path));

        let file_name = try!(file_path.file_name_or_err());
        Ok(dir_cache.check(file_name, stats))
    }

    pub fn insert(&mut self,
                  file_path: path::PathBuf,
                  stats: FileStats,
                  hash: dag::ObjectKey)
                  -> Result<()> {

        let dir_path = try!(file_path.parent_or_err());
        let dir_cache = try!(self.cache_for_dir(dir_path));

        let file_name = try!(file_path.file_name_or_err());
        Ok(dir_cache.insert_entry(file_name.into(), stats, hash))
    }

    pub fn flush(&mut self) { self.directory_caches.clear() }
}

#[cfg(test)]
mod test {
    use dag::parse_hash;
    use encodable;
    use rustc_serialize::json;
    use std::path;
    use super::*;
    use testutil;

    /// PathBufs are serialized as byte arrays instead of strings. Booo.
    #[test]
    fn test_serialize_pathbuf() {
        let obj = path::PathBuf::from("hello");
        let encoded = json::encode(&obj).unwrap();
        assert_eq!(encoded, "[104,101,108,108,111]");
        let decoded: path::PathBuf = json::decode(&encoded).unwrap();
        assert_eq!(decoded, obj);
    }

    #[test]
    fn test_serialize_filecache() {
        let mut obj = HashCache::new();
        obj.insert(encodable::PathBuf::from("patha/x"), CacheEntry{
            filestats: FileStats{
                mtime: encodable::SystemTime::unix_epoch_plus(120, 55),
                size: 12345,
            },
            hash: parse_hash("d3486ae9136e7856bc42212385ea797094475802"),
        });
        let encoded = json::encode(&obj).unwrap();
        let decoded: HashCache = json::decode(&encoded).unwrap();
        assert_eq!(decoded, obj);
    }

    #[test]
    fn test_hash_cache_file() {
        // Define some test values to use later
        let path0 = path::PathBuf::from("patha/x");
        let stats0 = FileStats {
            mtime: encodable::SystemTime::unix_epoch_plus(120, 55),
            size: 12345,
        };
        let hash0 = parse_hash("d3486ae9136e7856bc42212385ea797094475802");

        let path1 = path::PathBuf::from("pathb/y");
        let stats1 = FileStats {
            mtime: encodable::SystemTime::unix_epoch_plus(60, 22),
            size: 54321,
        };
        let hash1 = parse_hash("e030a4b3fdc15cdcbf9026d83b84c2b4b93309af");

        // Create temporary directory

        let tempdir = testutil::in_mem_tempdir("cache_test").unwrap();
        let cache_file_path = tempdir.path().join("cache");

        {
            // Open nonexistent cache file
            let mut cache_file = HashCacheFile::open(cache_file_path.clone())
                .expect("Open non-existent cache file");
            assert!(cache_file.is_empty(), "New cache should be empty");

            // Insert a value and let the destructor flush the file
            cache_file
                .insert_entry(path0.clone(), stats0.clone(), hash0.clone());
        }

        assert!(cache_file_path.is_file(), "New cache should be saved");

        {
            // Open the existing cache file
            let mut cache_file = HashCacheFile::open(cache_file_path.clone())
                .expect("Re-open cache file for firts time");
            assert!(!cache_file.is_empty(), "Read cache should not be empty");
            {
                let entry = cache_file.get(&path0).unwrap();

                assert_eq!(entry.filestats, stats0);
                assert_eq!(entry.hash, hash0);
            }

            // Insert another value and let the destructor flush the file
            cache_file
                .insert_entry(path1.clone(), stats1.clone(), hash1.clone());
        }

        {
            // Re-open the existing cache file
            let cache_file = HashCacheFile::open(cache_file_path.clone())
                .expect("Re-open cache file for second time");
            {
                let entry = cache_file.get(&path1).unwrap();

                assert_eq!(entry.filestats, stats1);
                assert_eq!(entry.hash, hash1);
            }
        }

    }
}
