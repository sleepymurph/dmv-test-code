use cache::CacheStatus;
use human_readable;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use super::*;

type PathKeyMap = BTreeMap<OsString, ObjectKey>;

wrapper_struct!{
    /// DAG Object representing a directory
    #[derive(Clone,Eq,PartialEq,Hash,Debug)]
    pub struct Tree(PathKeyMap);
}

impl Tree {
    pub fn new() -> Self { Tree(PathKeyMap::new()) }

    pub fn insert<P>(&mut self, name: P, hash: ObjectKey)
        where P: Into<OsString>
    {
        self.0.insert(name.into(), hash);
    }
}

/// Create and populate a Tree object
#[macro_export]
macro_rules! tree_object {
    ( $( $k:expr => $v:expr , )* ) => {
        map!{ $crate::dag::Tree::new(), $( $k=>$v, )* };
    }
}

const TREE_ENTRY_SEPARATOR: u8 = b'\n';

impl ObjectCommon for Tree {
    fn object_type(&self) -> ObjectType { ObjectType::Tree }
    fn content_size(&self) -> ObjectSize {
        self.0.iter().fold(0, |acc, x| {
            acc + KEY_SIZE_BYTES + x.0.as_os_str().len() + 1
        }) as ObjectSize
    }

    fn write_content(&self, writer: &mut io::Write) -> io::Result<()> {
        for entry in &self.0 {
            try!(writer.write(entry.1.as_ref()));
            try!(writer.write(entry.0.to_str().unwrap().as_bytes()));
            try!(writer.write(&[TREE_ENTRY_SEPARATOR]));
        }
        Ok(())
    }

    fn pretty_print(&self) -> String {
        use std::fmt::Write;
        let mut output = String::new();
        write!(&mut output,
               "Tree Index

Object content size:    {:>10}

",
               human_readable::human_bytes(self.content_size()))
            .unwrap();

        for entry in &self.0 {
            write!(&mut output,
                   "{:x} {}\n",
                   entry.1,
                   entry.0.to_str().unwrap())
                .unwrap();
        }
        output
    }
}

impl ReadObjectContent for Tree {
    fn read_content<R: io::BufRead>(reader: &mut R) -> Result<Self> {

        let mut tree = Tree::new();

        loop {
            // Read hash
            let mut hash_buf = [0u8; KEY_SIZE_BYTES];
            let bytes_read = try!(reader.read(&mut hash_buf));
            if bytes_read == 0 {
                break;
            }
            let hash = ObjectKey::from(hash_buf);

            // Read name
            let mut name_buf: Vec<u8> = Vec::new();
            try!(reader.read_until(TREE_ENTRY_SEPARATOR, &mut name_buf));
            name_buf.pop(); // Drop the string-ending separator
            let name = try!(String::from_utf8(name_buf));
            tree.insert(name, hash);
        }
        Ok(tree)
    }
}


type PartialMap = BTreeMap<OsString, HashedOrNot>;

/// An incomplete Tree object that requires some files to be hashed
#[derive(Clone,Eq,PartialEq,Hash,Debug)]
pub struct PartialTree(PartialMap);

impl_deref!(PartialTree => PartialMap);

/// For PartialTree: A child path that needs hashing
#[derive(Clone,Eq,PartialEq,Hash,Debug)]
pub enum UnhashedPath {
    /// The child path is a file, carry its size
    File(ObjectSize),
    /// The child path is a directory, carry its PartialTree
    Dir(PartialTree),
}

/// For PartialTree: A child path that may or may not need hashing
#[derive(Clone,Eq,PartialEq,Hash,Debug)]
pub enum HashedOrNot {
    /// The child path is a file with a known hash, carry the hash
    Hashed(ObjectKey),
    /// The child path is a file with unknown hash, carry the size
    UnhashedFile(ObjectSize),
    /// The child path is a directory
    Dir(PartialTree),
}

impl PartialTree {
    pub fn new() -> Self { PartialTree(PartialMap::new()) }

    /// Calculate the total size of all unhashed children
    ///
    /// How many bytes must be hashed to complete this Tree?
    pub fn unhashed_size(&self) -> ObjectSize {
        self.unhashed().map(|(_, unhashed)| unhashed.unhashed_size()).sum()
    }

    /// Insert a new child path
    ///
    /// Accepts any type that can be converted into a HashedOrNot.
    pub fn insert<P, T>(&mut self, path: P, st: T)
        where P: Into<OsString>,
              T: Into<HashedOrNot>
    {
        let st = st.into();
        match &st {
            &HashedOrNot::Dir(ref partial) if partial.is_empty() => return,
            _ => (),
        };
        self.0.insert(path.into(), st.into());
    }

    /// Get a map of unhashed children: path => UnhashedPath
    pub fn unhashed<'a>
        (&'a self)
         -> Box<Iterator<Item = (&'a OsString, &'a HashedOrNot)> + 'a> {
        Box::new(self.0.iter().filter(|&(_, entry)| match entry {
            &HashedOrNot::Hashed(_) => false,
            &HashedOrNot::UnhashedFile(_) => true,
            &HashedOrNot::Dir(_) => true,
        }))
    }

    /// Get a Tree from the known hashed children
    pub fn tree(&self) -> Tree {
        let mut tree = Tree::new();
        for (name, entry) in &self.0 {
            match entry {
                &HashedOrNot::Hashed(hash) => tree.insert(name, hash),
                _ => (),
            }
        }
        tree
    }

    /// Do all children have known hashes?
    ///
    /// Note that a PartialTree can be "incomplete," even if it has no files
    /// that need to be hashed. This can happen if one of the children is a
    /// PartialTree that is "complete." We may be able to calculate the hash of
    /// that subtree, but storing it as just a hash would loose the information
    /// we have about its children. So we should not do that until we can be
    /// sure that the tree has been stored in an object store.
    pub fn is_complete(&self) -> bool {
        for _ in self.unhashed() {
            return false;
        }
        true
    }
}

impl From<Tree> for PartialTree {
    fn from(t: Tree) -> Self {
        let mut partial = PartialTree::new();
        for (name, hash) in t.0 {
            partial.insert(name, HashedOrNot::Hashed(hash));
        }
        partial
    }
}

/// Create and populate a PartialTree object
#[macro_export]
macro_rules! partial_tree {
    ( $( $k:expr => $v:expr , )*) => {
        map!{ $crate::dag::PartialTree::new(), $( $k => $v, )* };
    }
}

impl IntoIterator for PartialTree {
    type Item = (OsString, HashedOrNot);
    type IntoIter = <BTreeMap<OsString, HashedOrNot> as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
}

// Conversions for HashedOrNot

impl From<CacheStatus> for HashedOrNot {
    fn from(s: CacheStatus) -> Self {
        use cache::CacheStatus::*;
        match s {
            Cached { hash } => HashedOrNot::Hashed(hash),
            Modified { size } |
            NotCached { size } => HashedOrNot::UnhashedFile(size),
        }
    }
}

impl From<PartialTree> for HashedOrNot {
    fn from(pt: PartialTree) -> Self { HashedOrNot::Dir(pt) }
}

impl From<UnhashedPath> for HashedOrNot {
    fn from(unhashed: UnhashedPath) -> Self {
        match unhashed {
            UnhashedPath::File(size) => HashedOrNot::UnhashedFile(size),
            UnhashedPath::Dir(partial) => HashedOrNot::Dir(partial),
        }
    }
}

impl From<ObjectKey> for HashedOrNot {
    fn from(hash: ObjectKey) -> Self { HashedOrNot::Hashed(hash) }
}

impl HashedOrNot {
    pub fn unhashed_size(&self) -> ObjectSize {
        use self::HashedOrNot::*;
        match self {
            &Hashed(_) => 0,
            &UnhashedFile(size) => size,
            &Dir(ref partial) => partial.unhashed_size(),
        }
    }
}

// Conversions for UnhashedPath

impl From<ObjectSize> for UnhashedPath {
    fn from(s: ObjectSize) -> Self { UnhashedPath::File(s) }
}

impl From<PartialTree> for UnhashedPath {
    fn from(pt: PartialTree) -> Self { UnhashedPath::Dir(pt) }
}

impl UnhashedPath {
    pub fn unhashed_size(&self) -> ObjectSize {
        match *self {
            UnhashedPath::File(size) => size,
            UnhashedPath::Dir(ref partial_tree) => partial_tree.unhashed_size(),
        }
    }
}

#[cfg(test)]
mod test {

    use std::ffi::OsString;
    use std::io;
    use super::super::*;
    use testutil;
    use testutil::rand::Rng;

    #[test]
    fn test_write_tree() {
        // Construct object
        let mut rng = testutil::TestRand::default();

        let object = tree_object!{
            "foo" => rng.gen::<ObjectKey>(),
            "bar" => rng.gen::<ObjectKey>(),
            "baz" => rng.gen::<ObjectKey>(),
        };

        // Write out
        let mut output: Vec<u8> = Vec::new();
        object.write_to(&mut output).expect("write out object");

        // Read in header
        let mut reader = io::BufReader::new(output.as_slice());
        let header = ObjectHeader::read_from(&mut reader).expect("read header");

        assert_eq!(header.object_type, ObjectType::Tree);
        assert_ne!(header.content_size, 0);

        // Read in object content
        let readobject = Tree::read_content(&mut reader)
            .expect("read object content");

        assert_eq!(readobject, object);
    }

    #[test]
    fn test_tree_sort_by_name() {
        let tree = tree_object!{
            "foo" => object_key(0),
            "bar" => object_key(2),
            "baz" => object_key(1),
        };

        let names: Vec<String> = tree.iter()
            .map(|ent| ent.0.to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["bar", "baz", "foo"]);
    }

    #[test]
    fn test_partial_tree() {

        // Create partial tree

        let mut partial = partial_tree!{
                "foo" => object_key(0),
                "bar" => object_key(2),
                "baz" => object_key(1),
                "fizz" => HashedOrNot::UnhashedFile(1024),
                "buzz" => partial_tree!{
                    "strange" => HashedOrNot::UnhashedFile(2048),
                },
        };

        assert_eq!(partial.get(&OsString::from("fizz")),
                   Some(&HashedOrNot::UnhashedFile(1024)));

        assert_eq!(partial.unhashed_size(), 3072);

        assert_eq!(partial.tree(),
                   tree_object!{
                        "foo" => object_key(0),
                        "bar" => object_key(2),
                        "baz" => object_key(1),
        });

        assert!(!partial.is_complete());

        // Begin adding hashes for incomplete objects

        partial.insert("buzz", object_key(3));
        assert_eq!(partial.get(&OsString::from("buzz")),
                   Some(&HashedOrNot::Hashed(object_key(3))));
        assert_eq!(partial.unhashed_size(), 1024);

        partial.insert("fizz", object_key(4));

        // Should be complete now

        assert!(partial.unhashed().next().is_none());
        assert!(partial.is_complete());
        assert_eq!(partial.unhashed_size(), 0);

        assert_eq!(partial.tree(),
                   tree_object!{
                        "foo" => object_key(0),
                        "bar" => object_key(2),
                        "baz" => object_key(1),
                        "fizz" => object_key(4),
                        "buzz" => object_key(3),
        });
    }

    #[test]
    fn test_partial_tree_with_zero_unhashed() {
        let partial = partial_tree!{
                "foo" => object_key(0),
                "bar" => partial_tree!{
                    "baz" => object_key(1),
                },
        };

        assert_eq!(partial.unhashed_size(), 0, "no files need to be hashed");
        assert_eq!(partial.is_complete(), false, "still incomplete");

        assert_eq!(partial.tree(),
                   tree_object!{
                        "foo" => object_key(0),
                   },
                   "not safe to take the tree value: it is missing the \
                    subtree");

        assert_eq!(partial.get(&OsString::from("bar")),
                   Some(&HashedOrNot::Dir(partial_tree!{
                        "baz" => object_key(1),
                   })),
                   "the nested PartialTree still holds information that \
                    would be lost if we replaced it with just a hash");
    }
}
