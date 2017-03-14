//! A filesystem parallel to the object_store, basis of a working directory

use cache::AllCaches;
use dag::ObjectKey;
use error::*;
use ignore::IgnoreList;
use object_store::ObjectStore;
use rolling_hash::read_file_objects;
use std::fs::*;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use walker::*;

/// A path (file or dir) plus metadata, a cached hash if any, and ignore flag
pub struct FileWalkNode {
    pub path: PathBuf,
    pub metadata: Metadata,
    pub hash: Option<ObjectKey>,
    pub ignored: bool,
}

/// Filesystem parallel to the ObjectStore, reads files plus cache/ignore info
pub struct FileStore {
    pub cache: AllCaches,
    pub ignored: IgnoreList,
}

impl FileStore {
    pub fn new() -> Self {
        FileStore {
            cache: AllCaches::new(),
            ignored: IgnoreList::default(),
        }
    }

    /// Store a single file and cache and return its hash
    pub fn hash_file(&mut self,
                     file_path: &Path,
                     object_store: &mut ObjectStore)
                     -> Result<ObjectKey> {
        let file = File::open(&file_path)?;
        let meta = file.metadata()?;
        let file = BufReader::new(file);

        if let Ok(Some(hash)) = self.cache.check(file_path, &meta) {
            debug!("Already hashed: {} {}", hash, file_path.display());
            return Ok(hash);
        }
        debug!("Hashing {}", file_path.display());

        let mut last_hash = None;
        for object in read_file_objects(file) {
            let object = object?;
            object_store.store_object(&object)?;
            last_hash = Some(object.hash().to_owned());
        }
        let last_hash = last_hash.expect("Iterator always emits objects");

        self.cache
            .insert(file_path.to_owned(), &meta, last_hash.to_owned())?;

        Ok(last_hash)
    }

    /// Extract a single file object and cache its hash
    pub fn extract_file(&mut self,
                        object_store: &ObjectStore,
                        hash: &ObjectKey,
                        path: &Path)
                        -> Result<()> {

        if path.is_file() {
            if let Some(ref c) = self.cache.check(path, &path.metadata()?)? {
                if c == hash {
                    debug!("Already at state: {} {}", hash, path.display());
                    return Ok(());
                }
            }
        } else if path.is_dir() {
            info!("Removing dir to extract file {} {}", hash, path.display());
            remove_dir_all(path)?;
        }

        let mut out_file = OpenOptions::new().write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        object_store.copy_blob_content(hash, &mut out_file)?;
        out_file.flush()?;

        self.cache.insert(path.to_owned(), &out_file.metadata()?, *hash)?;

        Ok(())
    }
}

impl NodeLookup<PathBuf, FileWalkNode> for FileStore {
    fn lookup_node(&self, path: PathBuf) -> Result<FileWalkNode> {
        let meta = path.metadata()?;
        Ok(FileWalkNode {
            hash: self.cache.check(&path, &meta)?,
            ignored: self.ignored.ignores(path.as_path()),
            path: path,
            metadata: meta,
        })
    }
}

impl NodeReader<FileWalkNode> for FileStore {
    fn read_children(&self,
                     node: &FileWalkNode)
                     -> Result<ChildMap<FileWalkNode>> {
        let mut children = ChildMap::new();
        for entry in read_dir(&node.path)? {
            let entry = entry?;
            let path = entry.path();
            let name = path.file_name_or_err()?
                .to_os_string()
                .into_string()
                .map_err(|e| format!("Bad UTF-8 in name: {:?}", e))?;
            let node = self.lookup_node(path.clone())?;
            children.insert(name, node);
        }
        Ok(children)
    }
}
