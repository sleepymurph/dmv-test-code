use cache::AllCaches;
use cache::FileStats;
use dag::ObjectHandle;
use dag::ObjectKey;
use dag::PartialTree;
use dag::UnhashedPath;
use error::*;
use ignore::IgnoreList;
use objectstore::ObjectStore;
use rollinghash::read_file_objects;
use std::fs::File;
use std::fs::OpenOptions;
use std::fs::read_dir;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;


pub fn hash_file(file_path: PathBuf,
                 cache: &mut AllCaches,
                 object_store: &mut ObjectStore)
                 -> Result<ObjectKey> {

    let file = try!(File::open(&file_path));
    let file_stats = FileStats::from(file.metadata()?);
    let file = BufReader::new(file);

    return_if_cached!(cache, &file_path, &file_stats);
    info!("Hashing {}", file_path.display());

    let mut last_hash = ObjectKey::zero();
    for object in read_file_objects(file) {
        last_hash = try!(object_store.store_object(&object?));
    }

    try!(cache.insert(file_path, file_stats, last_hash.clone()));

    Ok(last_hash)
}

pub fn extract_file(object_store: &ObjectStore,
                    hash: &ObjectKey,
                    file_path: &Path,
                    cache: &mut AllCaches)
                    -> Result<()> {

    return_if_cache_matches!(cache, file_path, hash);

    let mut out_file = OpenOptions::new().write(true)
        .create(true)
        .truncate(true)
        .open(file_path)?;

    try!(copy_blob_content(object_store, hash, &mut out_file));

    try!(out_file.flush());
    let file_stats = FileStats::from(out_file.metadata()?);
    try!(cache.insert(file_path.to_owned(), file_stats, hash.to_owned()));

    Ok(())
}

fn copy_blob_content(object_store: &ObjectStore,
                     hash: &ObjectKey,
                     writer: &mut Write)
                     -> Result<()> {

    let handle = try!(object_store.open_object(&hash));

    match handle {
        ObjectHandle::Blob(blob) => {
            debug!("Extracting blob {}", hash);
            blob.copy_content(writer)?;
        }
        ObjectHandle::ChunkedBlob(index) => {
            debug!("Reading ChunkedBlob {}", hash);
            let index = index.read_content()?;
            for offset in index.chunks {
                debug!("{}", offset);
                copy_blob_content(object_store, &offset.hash, writer)?;
            }
        }
        other => bail!("Expected a Blob or ChunkedBlob, got a {:?}", other),
    };
    Ok(())
}


/// Read filesystem to construct a PartialTree
pub fn dir_to_partial_tree(dir_path: &Path,
                           ignored: &IgnoreList,
                           cache: &mut AllCaches)
                           -> Result<PartialTree> {
    if dir_path.is_dir() {
        let mut partial = PartialTree::new();

        for entry in try!(read_dir(dir_path)) {
            let entry = try!(entry);

            let ch_path = entry.path();
            let ch_name = PathBuf::from(ch_path.file_name_or_err()?);
            let ch_metadata = try!(entry.metadata());

            if ignored.ignores(&ch_path) {
                continue;
            }

            if ch_metadata.is_file() {

                let cache_status =
                    try!(cache.check_with(&ch_path, &ch_metadata.into()));
                partial.insert(ch_name, cache_status);

            } else if ch_metadata.is_dir() {

                let subpartial =
                    try!(dir_to_partial_tree(&ch_path, &ignored, cache));
                partial.insert(ch_name, subpartial);

            } else {
                unimplemented!()
            }
        }

        Ok(partial)
    } else {
        bail!(ErrorKind::NotADirectory(dir_path.to_owned()))
    }
}

pub fn hash_partial_tree(dir_path: &Path,
                         mut partial: PartialTree,
                         cache: &mut AllCaches,
                         object_store: &mut ObjectStore)
                         -> Result<ObjectKey> {
    for (ch_name, unknown) in partial.unhashed().clone() {
        let ch_path = dir_path.join(&ch_name);

        let hash = match unknown {
            UnhashedPath::File(_) => hash_file(ch_path, cache, object_store),
            UnhashedPath::Dir(partial) => {
                hash_partial_tree(&ch_path, partial, cache, object_store)
            }
        };
        partial.insert(ch_name, hash?);
    }

    assert!(partial.is_complete());
    object_store.store_object(partial.tree())
}

#[cfg(test)]
mod test {
    use cache::AllCaches;
    use cache::CacheStatus;
    use dag::Blob;
    use dag::HashedOrNot;
    use dag::Object;
    use dag::ObjectCommon;
    use dag::ObjectKey;
    use dag::ObjectType;
    use ignore::IgnoreList;
    use objectstore::test::create_temp_repository;
    use rollinghash::CHUNK_TARGET_SIZE;
    use rollinghash::read_file_objects;
    use std::io::Cursor;
    use std::io::Read;
    use super::*;
    use testutil;

    #[test]
    fn test_hash_file_empty() {
        let (temp, mut object_store) = create_temp_repository().unwrap();
        let mut cache = AllCaches::new();
        let filepath = temp.path().join("foo");
        testutil::write_file(&filepath, "").unwrap();

        let hash = hash_file(filepath, &mut cache, &mut object_store).unwrap();

        let mut objfile = object_store.open_object_file(&hash).unwrap();
        let obj = Object::read_from(&mut objfile).unwrap();

        assert_eq!(Object::Blob(Blob::empty()), obj);
    }

    #[test]
    fn test_hash_file_small() {
        let (temp, mut object_store) = create_temp_repository().unwrap();
        let mut cache = AllCaches::new();
        let filepath = temp.path().join("foo");

        testutil::write_file(&filepath, "foo").unwrap();

        let hash = hash_file(filepath, &mut cache, &mut object_store).unwrap();

        let mut objfile = object_store.open_object_file(&hash).unwrap();
        let obj = Object::read_from(&mut objfile).unwrap();

        assert_eq!(Object::Blob(Blob::from("foo")), obj);
    }

    #[test]
    fn test_hash_file_chunked() {
        let (temp, mut object_store) = create_temp_repository().unwrap();
        let mut cache = AllCaches::new();
        let filepath = temp.path().join("foo");
        let filesize = 3 * CHUNK_TARGET_SIZE as u64;

        let mut rng = testutil::TestRand::default();
        testutil::write_file(&filepath, rng.take(filesize)).unwrap();

        let hash = hash_file(filepath, &mut cache, &mut object_store).unwrap();

        let mut obj = object_store.open_object_file(&hash).unwrap();
        let obj = Object::read_from(&mut obj).unwrap();

        if let Object::ChunkedBlob(chunked) = obj {
            assert_eq!(chunked.total_size, filesize);
            assert_eq!(chunked.chunks.len(), 5);

            for chunkrecord in chunked.chunks {
                let mut obj = object_store.open_object_file(&chunkrecord.hash)
                    .unwrap();
                let obj = Object::read_from(&mut obj).unwrap();
                assert_eq!(obj.object_type(), ObjectType::Blob);
                assert_eq!(obj.content_size(), chunkrecord.size);
            }

        } else {
            panic!("Not a ChunkedBlob: {:?}", obj);
        }

    }

    #[test]
    fn test_extract_file_object_not_found() {
        let (temp, object_store) = create_temp_repository().unwrap();
        let mut cache = AllCaches::new();
        let out_file = temp.path().join("foo");
        let hash = Blob::from("12345").calculate_hash();

        let result = extract_file(&object_store, &hash, &out_file, &mut cache);

        match result {
            Err(Error(ErrorKind::ObjectNotFound(err_hash), _)) => {
                assert_eq!(err_hash, hash)
            }
            _ => panic!("Unexpected result {:?}", result),
        }
    }

    #[test]
    fn test_extract_file_single_blob() {
        let (temp, mut object_store) = create_temp_repository().unwrap();
        let mut cache = AllCaches::new();

        let blob = Blob::from("12345");
        let hash = object_store.store_object(&blob).unwrap();

        let out_file = temp.path().join("foo");
        extract_file(&object_store, &hash, &out_file, &mut cache).unwrap();

        let out_content = testutil::read_file_to_string(&out_file).unwrap();
        assert_eq!(out_content, "12345");

        assert_eq!(cache.check(&out_file).unwrap(),
                   CacheStatus::Cached { hash: hash },
                   "Cache should be primed with extracted file's hash");
    }

    #[test]
    fn test_extract_file_multi_chunks() {
        let (temp, mut object_store) = create_temp_repository().unwrap();
        let mut cache = AllCaches::new();

        let mut rng = testutil::TestRand::default();
        let filesize = 3 * CHUNK_TARGET_SIZE as u64;
        let mut in_file = Vec::new();
        rng.take(filesize).read_to_end(&mut in_file).unwrap();

        let mut hash = ObjectKey::zero();
        for object in read_file_objects(Cursor::new(&in_file)) {
            hash = object_store.store_object(&object.unwrap()).unwrap();
        }

        let out_file = temp.path().join("foo");
        extract_file(&object_store, &hash, &out_file, &mut cache).unwrap();

        assert_eq!(out_file.metadata().unwrap().len(), filesize);

        let out_content = testutil::read_file_to_end(&out_file).unwrap();
        assert!(out_content == in_file);

        assert_eq!(cache.check(&out_file).unwrap(),
                   CacheStatus::Cached { hash: hash },
                   "Cache should be primed with extracted file's hash");
    }

    #[test]
    fn test_store_directory_shallow() {
        let (temp, mut object_store) = create_temp_repository().unwrap();
        let ignored = IgnoreList::default();
        let mut cache = AllCaches::new();
        let wd_path = temp.path().join("work_dir");

        write_files!{
            wd_path;
            "foo" => "123",
            "bar" => "1234",
            "baz" => "12345",
        };

        let expected_partial = partial_tree!{
            "foo" => HashedOrNot::UnhashedFile(3),
            "bar" => HashedOrNot::UnhashedFile(4),
            "baz" => HashedOrNot::UnhashedFile(5),
        };

        let expected_tree = tree_object!{
            "foo" => Blob::from("123").calculate_hash(),
            "bar" => Blob::from("1234").calculate_hash(),
            "baz" => Blob::from("12345").calculate_hash(),
        };

        // Build partial tree

        let partial = dir_to_partial_tree(&wd_path, &ignored, &mut cache)
            .unwrap();
        assert_eq!(partial, expected_partial);
        assert_eq!(partial.unhashed_size(), 12);

        // Hash and store files

        let hash =
            hash_partial_tree(&wd_path, partial, &mut cache, &mut object_store)
                .unwrap();

        let mut file = object_store.open_object_file(&hash).unwrap();
        let tree = Object::read_from(&mut file).unwrap();

        assert_eq!(tree, Object::Tree(expected_tree.clone()));

        // Check that children are stored

        for (name, hash) in expected_tree.iter() {
            assert!(object_store.has_object(&hash),
                    "Object for '{}' was not stored",
                    name.display());
        }

        // Check again that files are cached now

        let partial = dir_to_partial_tree(&wd_path, &ignored, &mut cache)
            .unwrap();
        assert!(partial.is_complete());
        assert_eq!(partial.tree(), &expected_tree);
    }

    #[test]
    fn test_store_directory_recursive() {
        let (temp, mut object_store) = create_temp_repository().unwrap();
        let ignored = IgnoreList::default();
        let mut cache = AllCaches::new();
        let wd_path = temp.path().join("work_dir");

        write_files!{
            wd_path;
            "foo" => "123",
            "level1/bar" => "1234",
            "level1/level2/baz" => "12345",
        };

        let expected_partial = partial_tree!{
            "foo" => HashedOrNot::UnhashedFile(3),
            "level1" => partial_tree!{
                "bar" => HashedOrNot::UnhashedFile(4),
                "level2" => partial_tree!{
                    "baz" => HashedOrNot::UnhashedFile(5),
                },
            },
        };

        let expected_tree = tree_object!{
            "foo" => Blob::from("123").calculate_hash(),
            "level1" => tree_object!{
                "bar" => Blob::from("1234").calculate_hash(),
                "level2" => tree_object!{
                    "baz" => Blob::from("12345").calculate_hash(),
                }.calculate_hash(),
            }.calculate_hash(),
        };

        let expected_cached_partial = partial_tree!{
            "foo" => Blob::from("123").calculate_hash(),
            "level1" => partial_tree!{
                "bar" => Blob::from("1234").calculate_hash(),
                "level2" => partial_tree!{
                    "baz" => Blob::from("12345").calculate_hash(),
                },
            },
        };

        let deepest_child_hash = Blob::from("12345").calculate_hash();

        // Build partial tree

        let partial = dir_to_partial_tree(&wd_path, &ignored, &mut cache)
            .unwrap();
        assert_eq!(partial, expected_partial);
        assert_eq!(partial.unhashed_size(), 12);

        // Hash and store files

        let hash =
            hash_partial_tree(&wd_path, partial, &mut cache, &mut object_store)
                .unwrap();

        let mut file = object_store.open_object_file(&hash).unwrap();
        let tree = Object::read_from(&mut file).unwrap();

        assert_eq!(tree, Object::Tree(expected_tree.clone()));

        // Check that deepest child is stored

        assert!(object_store.has_object(&deepest_child_hash),
                "Object for deepest child was not stored");

        // Check again that files are cached now

        let partial = dir_to_partial_tree(&wd_path, &ignored, &mut cache)
            .unwrap();
        assert_eq!(partial.unhashed_size(), 0);
        assert!(!partial.is_complete());
        assert_eq!(partial, expected_cached_partial);
    }

    #[test]
    fn test_ignore_objectstore_dir() {
        let (temp, mut object_store) = create_temp_repository().unwrap();
        let mut ignored = IgnoreList::default();
        ignored.insert(object_store.path());
        let mut cache = AllCaches::new();
        let wd_path = temp.path();

        write_files!{
            wd_path;
            "foo" => "123",
            "level1/bar" => "1234",
            "level1/level2/baz" => "12345",
        };

        let expected_partial = partial_tree!{
            "foo" => HashedOrNot::UnhashedFile(3),
            "level1" => partial_tree!{
                "bar" => HashedOrNot::UnhashedFile(4),
                "level2" => partial_tree!{
                    "baz" => HashedOrNot::UnhashedFile(5),
                },
            },
        };

        let expected_tree = tree_object!{
            "foo" => Blob::from("123").calculate_hash(),
            "level1" => tree_object!{
                "bar" => Blob::from("1234").calculate_hash(),
                "level2" => tree_object!{
                    "baz" => Blob::from("12345").calculate_hash(),
                }.calculate_hash(),
            }.calculate_hash(),
        };

        let expected_cached_partial = partial_tree!{
            "foo" => Blob::from("123").calculate_hash(),
            "level1" => partial_tree!{
                "bar" => Blob::from("1234").calculate_hash(),
                "level2" => partial_tree!{
                    "baz" => Blob::from("12345").calculate_hash(),
                },
            },
        };

        // Build partial tree

        let partial = dir_to_partial_tree(&wd_path, &ignored, &mut cache)
            .unwrap();
        assert_eq!(partial, expected_partial);

        // Hash and store files

        let hash =
            hash_partial_tree(&wd_path, partial, &mut cache, &mut object_store)
                .unwrap();

        let mut file = object_store.open_object_file(&hash).unwrap();
        let tree = Object::read_from(&mut file).unwrap();

        assert_eq!(tree, Object::Tree(expected_tree.clone()));

        // Flush cache files
        cache.flush();

        // Build partial tree again -- make sure it doesn't pick up cache files

        let partial = dir_to_partial_tree(&wd_path, &ignored, &mut cache)
            .unwrap();
        assert_eq!(partial, expected_cached_partial);
    }

}
