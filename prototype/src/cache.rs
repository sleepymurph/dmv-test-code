use dag;
use encodable;
use rustc_serialize::json;
use std::collections;
use std::convert;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::ops;
use std::path;

#[derive(Clone,Eq,PartialEq,Debug)]
pub enum CacheStatus {
    NotCached { size: dag::ObjectSize },
    Modified { size: dag::ObjectSize },
    Cached { hash: dag::ObjectKey },
}

#[derive(Clone,Eq,PartialEq,Debug)]
pub struct HashCache(CacheMap);

pub type CacheMap = collections::HashMap<encodable::PathBuf, CacheEntry>;

#[derive(Clone,Eq,PartialEq,Debug,RustcEncodable,RustcDecodable)]
pub struct CacheEntry {
    pub filestats: FileStats,
    pub hash: dag::ObjectKey,
}

/// Status used to detect file changes
#[derive(Clone,Eq,PartialEq,Debug,RustcEncodable,RustcDecodable)]
pub struct FileStats {
    size: dag::ObjectSize,
    mtime: encodable::SystemTime,
}


/// A file-backed cache that saves updates on drop
pub struct HashCacheFile {
    /// Path to the file that stores the cache
    cache_file_path: path::PathBuf,
    /// Open File object that stores the cache
    cache_file: fs::File,
    /// The cache map itself
    cache: HashCache,
}

// HashCache

impl HashCache {
    pub fn new() -> Self {
        HashCache(CacheMap::new())
    }

    pub fn insert(&mut self,
                  file_path: path::PathBuf,
                  file_stats: FileStats,
                  hash: dag::ObjectKey) {
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
}

impl ops::Deref for HashCache {
    type Target = CacheMap;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl convert::AsRef<CacheMap> for HashCache {
    fn as_ref(&self) -> &CacheMap {
        &self.0
    }
}

impl convert::AsMut<CacheMap> for HashCache {
    fn as_mut(&mut self) -> &mut CacheMap {
        &mut self.0
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

impl HashCacheFile {
    pub fn open(cache_file_path: path::PathBuf) -> CacheResult<Self> {
        let cache_file_exists = cache_file_path.exists();

        let mut cache_file = try!(fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&cache_file_path));

        let cache_map = if cache_file_exists {
            let mut json_str = String::new();
            try!(cache_file.read_to_string(&mut json_str));
            try!(json::decode(&json_str)
                .map_err(|e| CacheError::from_json_decoder_error(e, json_str)))
        } else {
            CacheMap::new()
        };

        Ok(HashCacheFile {
            cache_file_path: cache_file_path,
            cache_file: cache_file,
            cache: HashCache(cache_map),
        })
    }

    pub fn flush(&mut self) -> CacheResult<()> {
        use std::io::Seek;

        let encoded = try!(json::encode(&self.cache.0).map_err(|e| {
            CacheError::from_json_encoder_error(e, self.cache.0.clone())
        }));
        try!(self.cache_file.seek(io::SeekFrom::Start(0)));
        try!(self.cache_file.set_len(0));
        try!(self.cache_file.write_all(encoded.as_bytes()));
        Ok(())
    }
}

impl ops::Drop for HashCacheFile {
    fn drop(&mut self) {
        self.flush().expect("Could not flush hash file")
    }
}

impl ops::Deref for HashCacheFile {
    type Target = HashCache;
    fn deref(&self) -> &HashCache {
        &self.cache
    }
}

impl convert::AsMut<HashCache> for HashCacheFile {
    fn as_mut(&mut self) -> &mut HashCache {
        &mut self.cache
    }
}

// --------------------------------------------------
// Errors

type CacheResult<T> = Result<T, CacheError>;

#[derive(Debug)]
pub enum CacheError {
    CorruptJson {
        cause: json::DecoderError,
        bad_json: String,
    },
    SerializeError {
        cause: json::EncoderError,
        bad_cache: CacheMap,
    },
    IoError { cause: io::Error },
}

impl CacheError {
    fn from_json_decoder_error(err: json::DecoderError,
                               bad_json: String)
                               -> CacheError {
        CacheError::CorruptJson {
            cause: err,
            bad_json: bad_json,
        }
    }

    fn from_json_encoder_error(err: json::EncoderError,
                               bad_cache: CacheMap)
                               -> CacheError {
        CacheError::SerializeError {
            cause: err,
            bad_cache: bad_cache,
        }
    }
}

impl From<io::Error> for CacheError {
    fn from(err: io::Error) -> CacheError {
        CacheError::IoError { cause: err }
    }
}


#[cfg(test)]
mod test {
    use dag;
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
        obj.as_mut().insert(encodable::PathBuf::from("patha/x"), CacheEntry{
            filestats: FileStats{
                mtime: encodable::SystemTime::unix_epoch_plus(120, 55),
                size: 12345,
            },
            hash: dag::ObjectKey
                ::from_hex("d3486ae9136e7856bc42212385ea797094475802").unwrap(),
        });
        let encoded = json::encode(&obj.0).unwrap();
        let decoded: CacheMap = json::decode(&encoded).unwrap();
        assert_eq!(HashCache(decoded), obj);
    }

    #[test]
    fn test_hash_cache_file() {
        // Define some test values to use later
        let path0 = path::PathBuf::from("patha/x");
        let stats0 = FileStats {
            mtime: encodable::SystemTime::unix_epoch_plus(120, 55),
            size: 12345,
        };
        let hash0 =
            dag::ObjectKey::from_hex("d3486ae9136e7856bc42212385ea797094475802")
                .unwrap();

        let path1 = path::PathBuf::from("pathb/y");
        let stats1 = FileStats {
            mtime: encodable::SystemTime::unix_epoch_plus(60, 22),
            size: 54321,
        };
        let hash1 =
            dag::ObjectKey::from_hex("e030a4b3fdc15cdcbf9026d83b84c2b4b93309af")
                .unwrap();

        // Create temporary directory

        let tempdir = testutil::in_mem_tempdir("cache_test").unwrap();
        let cache_file_path = tempdir.path().join("cache");

        {
            // Open nonexistent cache file
            let mut cache_file = HashCacheFile::open(cache_file_path.clone())
                .expect("Open non-existent cache file");
            assert!(cache_file.is_empty(), "New cache should be empty");

            // Insert a value and let the destructor flush the file
            cache_file.as_mut()
                .insert(path0.clone(), stats0.clone(), hash0.clone());
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
            cache_file.as_mut()
                .insert(path1.clone(), stats1.clone(), hash1.clone());
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
