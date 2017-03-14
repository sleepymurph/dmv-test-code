//! Functionality for transfering files between filesystem and object store

use dag::ObjectKey;
use dag::Tree;
use error::*;
use file_store::FileStore;
use file_store::FileWalkNode;
use human_readable::human_bytes;
use ignore::IgnoreList;
use object_store::ObjectStore;
use object_store::ObjectWalkNode;
use status::HashPlan;
use status::Status;
use std::collections::BTreeMap;
use std::fs::create_dir;
use std::fs::remove_file;
use std::path::Path;
use std::path::PathBuf;
use walker::*;


/// Combine a FileStore and an ObjectStore to provide transfers between them
pub struct FsTransfer {
    pub object_store: ObjectStore,
    pub file_store: FileStore,
}

impl_deref_mut!(FsTransfer => ObjectStore, object_store);

impl FsTransfer {
    pub fn with_object_store(object_store: ObjectStore) -> Self {
        let mut ignored = IgnoreList::default();
        ignored.insert(object_store.path());

        FsTransfer {
            object_store: object_store,
            file_store: FileStore::new(),
        }
    }

    pub fn with_repo_path(repo_path: PathBuf) -> Result<Self> {
        Ok(FsTransfer::with_object_store(ObjectStore::open(repo_path)?))
    }

    /// Check, hash, and store a file or directory
    pub fn hash_path(&mut self, path: &Path) -> Result<ObjectKey> {
        debug!("Hashing object, with framework");
        let no_answer_err = || Error::from("Nothing to hash (all ignored?)");
        let hash_plan = self.file_store
            .walk_handle(&mut FsOnlyPlanBuilder, path.to_owned())?
            .ok_or_else(&no_answer_err)?;
        if hash_plan.unhashed_size() > 0 {
            stderrln!("{} to hash. Hashing...",
                      human_bytes(hash_plan.unhashed_size()));
        }
        hash_plan.walk(&mut HashAndStoreOp { fs_transfer: self })?
            .ok_or_else(&no_answer_err)
    }

    /// Extract a file or directory from the object store to the filesystem
    pub fn extract_object(&mut self,
                          hash: &ObjectKey,
                          path: &Path)
                          -> Result<()> {

        let mut op = ExtractObjectOp {
            file_store: &mut self.file_store,
            object_store: &self.object_store,
            extract_root: path,
        };

        self.object_store
            .walk_handle(&mut op, *hash)
            .chain_err(|| {
                format!("Could not extract {} to {}", hash, path.display())
            })?;
        Ok(())
    }

    fn hash_file(&mut self, file_path: &Path) -> Result<ObjectKey> {
        self.file_store.hash_file(file_path, &mut self.object_store)
    }
}



/// An operation that walks files to build a HashPlan
///
/// Only considers ignore and cache status. See FsObjComparePlanBuilder for an
/// operation that compares to a previous commit/tree.
pub struct FsOnlyPlanBuilder;

impl FsOnlyPlanBuilder {
    fn status(&self, node: &FileWalkNode) -> Status {
        match node {
            &FileWalkNode { ignored: true, .. } => Status::Ignored,
            _ => Status::Add,
        }
    }
}

impl WalkOp<FileWalkNode> for FsOnlyPlanBuilder {
    type VisitResult = HashPlan;

    fn should_descend(&mut self, _ps: &PathStack, node: &FileWalkNode) -> bool {
        node.metadata.is_dir() && self.status(node).is_included()
    }
    fn no_descend(&mut self,
                  _ps: &PathStack,
                  node: FileWalkNode)
                  -> Result<Option<Self::VisitResult>> {
        Ok(Some(HashPlan {
            status: self.status(&node),
            path: node.path,
            is_dir: node.metadata.is_dir(),
            hash: node.hash,
            size: node.metadata.len(),
            children: BTreeMap::new(),
        }))
    }
    fn post_descend(&mut self,
                    ps: &PathStack,
                    node: FileWalkNode,
                    children: ChildMap<Self::VisitResult>)
                    -> Result<Option<Self::VisitResult>> {
        self.no_descend(ps, node).map(|result| {
            result.map(|mut plan| {
                plan.children = children;
                plan
            })
        })
    }
}



/// An operation that compares files to a previous commit to build a HashPlan
///
/// Walks a filesystem tree and a Tree object in parallel, comparing them and
/// building a HashPlan. This is the basis of the status command and the first
/// step of a commit.
pub struct FsObjComparePlanBuilder;

type CompareNode = (Option<FileWalkNode>, Option<ObjectWalkNode>);

impl FsObjComparePlanBuilder {
    fn status(node: &CompareNode) -> Status {
        let (path_exists, path_hash, path_is_ignored) = match node.0 {
            Some(ref p) => (true, p.hash, p.ignored),
            None => (false, None, true),
        };
        let (obj_exists, obj_hash) = match node.1 {
            Some(ref o) => (true, Some(o.0)),
            None => (false, None),
        };
        match (path_exists, obj_exists, path_hash, obj_hash) {
            (true, true, Some(a), Some(b)) if a == b => Status::Unchanged,
            (true, true, Some(_), Some(_)) => Status::Modified,
            (true, true, _, _) => Status::MaybeModified,

            (true, false, _, _) if path_is_ignored => Status::Ignored,
            (true, false, _, _) => Status::Untracked,

            (false, true, _, _) => Status::Offline,

            (false, false, _, _) => unreachable!(),
        }
    }
}

impl WalkOp<CompareNode> for FsObjComparePlanBuilder {
    type VisitResult = HashPlan;

    fn should_descend(&mut self, _ps: &PathStack, node: &CompareNode) -> bool {
        let path_is_dir = match node.0 {
            Some(ref pwn) => pwn.metadata.is_dir(),
            None => false,
        };
        path_is_dir && Self::status(&node).is_included()
    }
    fn no_descend(&mut self,
                  _ps: &PathStack,
                  node: CompareNode)
                  -> Result<Option<Self::VisitResult>> {
        let status = Self::status(&node);
        match node {
            (Some(path), _) => {
                Ok(Some(HashPlan {
                    status: status,
                    path: path.path,
                    is_dir: path.metadata.is_dir(),
                    hash: path.hash,
                    size: path.metadata.len(),
                    children: BTreeMap::new(),
                }))
            }
            (None, Some(obj)) => {
                Ok(Some(HashPlan {
                    status: status,
                    hash: Some(obj.0),
                    path: "".into(),
                    is_dir: false,
                    size: 0,
                    children: BTreeMap::new(),
                }))
            }
            (None, None) => unreachable!(),
        }
    }
    fn post_descend(&mut self,
                    ps: &PathStack,
                    node: CompareNode,
                    children: ChildMap<Self::VisitResult>)
                    -> Result<Option<Self::VisitResult>> {
        self.no_descend(ps, node).map(|result| {
            result.map(|mut plan| {
                plan.children = children;
                plan
            })
        })
    }
}



/// An operation that walks a HashPlan to hash and store the files as a Tree
pub struct HashAndStoreOp<'a> {
    fs_transfer: &'a mut FsTransfer,
}

impl<'a> WalkOp<&'a HashPlan> for HashAndStoreOp<'a> {
    type VisitResult = ObjectKey;

    fn should_descend(&mut self, _ps: &PathStack, node: &&HashPlan) -> bool {
        node.is_dir && node.status.is_included()
    }

    fn no_descend(&mut self,
                  _ps: &PathStack,
                  node: &HashPlan)
                  -> Result<Option<Self::VisitResult>> {
        match (node.status.is_included(), node.hash) {
            (false, _) => Ok(None),
            (true, Some(hash)) => Ok(Some(hash)),
            (true, None) => {
                let hash = self.fs_transfer.hash_file(node.path.as_path())?;
                Ok(Some(hash))
            }
        }
    }

    fn post_descend(&mut self,
                    _ps: &PathStack,
                    _node: &HashPlan,
                    children: ChildMap<Self::VisitResult>)
                    -> Result<Option<Self::VisitResult>> {
        if children.is_empty() {
            return Ok(None);
        }
        let mut tree = Tree::new();
        for (name, hash) in children {
            tree.insert(name, hash);
        }
        let hash = self.fs_transfer.store_object(&tree)?;
        Ok(Some(hash))
    }
}



/// An operation that walks a Tree (or Commit) object to extract it to disk
pub struct ExtractObjectOp<'a> {
    file_store: &'a mut FileStore,
    object_store: &'a ObjectStore,
    extract_root: &'a Path,
}

impl<'a> ExtractObjectOp<'a> {
    fn abs_path(&self, ps: &PathStack) -> PathBuf {
        let mut abs_path = self.extract_root.to_path_buf();
        for path in ps {
            abs_path.push(path);
        }
        abs_path
    }
}

impl<'a> WalkOp<ObjectWalkNode> for ExtractObjectOp<'a> {
    type VisitResult = ();

    fn should_descend(&mut self,
                      _ps: &PathStack,
                      node: &ObjectWalkNode)
                      -> bool {
        node.1.is_treeish()
    }

    fn pre_descend(&mut self,
                   ps: &PathStack,
                   _node: &ObjectWalkNode)
                   -> Result<()> {
        let dir_path = self.abs_path(ps);
        if !dir_path.is_dir() {
            if dir_path.exists() {
                remove_file(&dir_path)?;
            }
            create_dir(&dir_path)?;
        }
        Ok(())
    }

    fn no_descend(&mut self,
                  ps: &PathStack,
                  node: ObjectWalkNode)
                  -> Result<Option<Self::VisitResult>> {
        let abs_path = self.abs_path(ps);
        self.file_store
            .extract_file(self.object_store, &node.0, abs_path.as_path())?;
        Ok(None)
    }
}



#[cfg(test)]
mod test {
    use cache::CacheStatus;
    use dag::Blob;
    use dag::ObjectCommon;
    use dag::ObjectType;
    use hamcrest::prelude::*;
    use rolling_hash::CHUNK_TARGET_SIZE;
    use std::fs::create_dir_all;
    use super::*;
    use testutil;
    use testutil::tempdir::TempDir;

    fn create_temp_repo(dir_name: &str) -> (TempDir, FsTransfer) {
        let temp = in_mem_tempdir!();
        let repo_path = temp.path().join(dir_name);
        let fs_transfer = FsTransfer::with_repo_path(repo_path).unwrap();
        (temp, fs_transfer)
    }

    fn do_store_single_file_test(in_file: &[u8],
                                 expected_object_type: ObjectType) {

        let (temp, mut fs_transfer) = create_temp_repo("object_store");

        // Write input file to disk
        let filepath = temp.path().join("foo");
        testutil::write_file(&filepath, in_file).unwrap();

        // Hash input file
        let hash = fs_transfer.hash_path(&filepath).unwrap();

        // Check the object type
        let obj = fs_transfer.open_object(&hash).unwrap();
        assert_eq!(obj.header().object_type, expected_object_type);

        // Extract the object
        let out_file = temp.path().join("bar");
        fs_transfer.extract_object(&hash, &out_file).unwrap();

        // Compare input and output
        assert_eq!(out_file.metadata().unwrap().len(), in_file.len() as u64);
        let out_content = testutil::read_file_to_end(&out_file).unwrap();
        assert!(out_content.as_slice() == in_file, "file contents differ");

        // Make sure the output is cached
        assert_eq!(fs_transfer.file_store
                       .cache
                       .status(&out_file, &out_file.metadata().unwrap())
                       .unwrap(),
                   CacheStatus::Cached(hash),
                   "Cache should be primed with extracted file's hash");
    }

    #[test]
    fn test_hash_file_empty() {
        do_store_single_file_test(&Vec::new(), ObjectType::Blob);
    }

    #[test]
    fn test_hash_file_small() {
        do_store_single_file_test("foo".as_bytes(), ObjectType::Blob);
    }

    #[test]
    fn test_hash_file_chunked() {
        let filesize = 3 * CHUNK_TARGET_SIZE;
        let in_file = testutil::TestRand::default().gen_byte_vec(filesize);
        do_store_single_file_test(&in_file, ObjectType::ChunkedBlob);
    }

    #[test]
    fn test_extract_object_object_not_found() {
        let (temp, mut fs_transfer) = create_temp_repo("object_store");

        let out_file = temp.path().join("foo");
        let hash = Blob::from("12345").calculate_hash();

        let result = fs_transfer.extract_object(&hash, &out_file);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_overwrite_policy() {
        let (temp, mut fs_transfer) = create_temp_repo("object_store");
        let wd_path = temp.path().join("work_dir");

        let source = wd_path.join("in_file");
        testutil::write_file(&source, "in_file content").unwrap();
        let hash = fs_transfer.hash_file(source.as_path()).unwrap();


        // File vs cached file
        let target = wd_path.join("cached_file");
        testutil::write_file(&target, "cached_file content").unwrap();
        fs_transfer.hash_file(target.as_path()).unwrap();

        fs_transfer.extract_object(&hash, &target).unwrap();
        let content = testutil::read_file_to_string(&target).unwrap();
        assert_that!(&content, equal_to("in_file content"));


        // File vs uncached file
        let target = wd_path.join("uncached_file");
        testutil::write_file(&target, "uncached_file content").unwrap();

        fs_transfer.extract_object(&hash, &target).unwrap();
        let content = testutil::read_file_to_string(&target).unwrap();
        assert_that!(&content, equal_to("in_file content"));


        // File vs empty dir
        let target = wd_path.join("empty_dir");
        create_dir_all(&target).unwrap();

        fs_transfer.extract_object(&hash, &target).unwrap();
        let content = testutil::read_file_to_string(&target).unwrap();
        assert_that!(&content, equal_to("in_file content"));


        // File vs non-empty dir
        let target = wd_path.join("dir");
        write_files!{
            &target;
            "dir_file" => "dir_file content",
        };

        fs_transfer.extract_object(&hash, &target).unwrap();
        let content = testutil::read_file_to_string(&target).unwrap();
        assert_that!(&content, equal_to("in_file content"));
    }

    #[test]
    fn test_extract_directory_clobber_file() {
        let (temp, mut fs_transfer) = create_temp_repo("object_store");
        let wd_path = temp.path().join("work_dir");

        let source = wd_path.join("in_dir");
        write_files!{
                source;
                "file1" => "dir/file1 content",
                "file2" => "dir/file2 content",
        };

        let hash = fs_transfer.hash_path(&source).unwrap();

        // Dir vs cached file
        let target = wd_path.join("cached_file");
        testutil::write_file(&target, "cached_file content").unwrap();
        fs_transfer.hash_file(target.as_path()).unwrap();

        fs_transfer.extract_object(&hash, &target).unwrap();
        assert_that!(&target, existing_dir());


        // Dir vs uncached file
        let target = wd_path.join("uncached_file");
        testutil::write_file(&target, "uncached_file content").unwrap();
        fs_transfer.hash_file(target.as_path()).unwrap();

        fs_transfer.extract_object(&hash, &target).unwrap();
        assert_that!(&target, existing_dir());


        // Dir vs empty dir
        let target = wd_path.join("empty_dir");
        create_dir_all(&target).unwrap();

        fs_transfer.extract_object(&hash, &target).unwrap();
        assert_that!(&target, existing_dir());
        assert_that!(&target.join("file1"), existing_file());


        // Dir vs non-empty dir
        let target = wd_path.join("non_empty_dir");
        write_files!{
            target;
            "target_file1" => "target_file1 content",
        };

        fs_transfer.extract_object(&hash, &target).unwrap();
        assert_that!(&target, existing_dir());
        assert_that!(&target.join("file1"), existing_file());
        assert_that!(&target.join("file2"), existing_file());
        assert_that!(&target.join("target_file1"), existing_file());
    }
}
