use error::*;
use std::io::BufRead;
use std::io::Write;
use std::marker::PhantomData;
use super::*;

/// A handle to an open Object file, with the header parsed but not the content
///
/// After destructuring, each inner handle will have a parse() method that will
/// read the content as the appropriate object.
///
/// Specific object types may have other options. For instance, a BlobHandle can
/// copy its data in a streaming fashion without loading it all into memory.
///
/// That decision of streaming vs loading is the main motivation behind this
/// type.
///
/// ```
/// use prototypelib::dag::{ObjectHandle, Object, ObjectCommon, Blob};
/// use std::io::Cursor;
///
/// // Create a test blob "file"
/// let mut file = Vec::new();
/// Blob::empty().write_to(&mut file);
/// let file = Box::new(Cursor::new(file));
///
/// // Read file
/// let handle = ObjectHandle::read_header(file).unwrap();
/// match handle {
///     ObjectHandle::Blob(bh) => {
///         // Blob can copy content as a stream
///         let mut copy = Vec::<u8>::new();
///         bh.copy_content(&mut copy).unwrap();
///     }
///     ObjectHandle::Tree(th) => {
///         // Others can be parsed in a type-safe manner
///         let tree = th.parse().unwrap();
///         for (k, v) in tree.iter() {
///             // ...
///         }
///     }
///     other => {
///         // Can be parsed to an Object enum as well
///         let object = other.parse().unwrap();
///         object.pretty_print();
///     }
/// }
/// ```
///
pub enum ObjectHandle {
    Blob(RawHandle<Blob>),
    ChunkedBlob(RawHandle<ChunkedBlob>),
    Tree(RawHandle<Tree>),
    Commit(RawHandle<Commit>),
}

/// Type-differentiated object handle, inner type of each ObjectHandle variant
///
/// All have a `parse` method which reads the rest of the file to give the
/// appropriate DAG object.
///
/// Specific types may have additional methods, such as the `copy_content`
/// method when working with a Blob.
///
pub struct RawHandle<O: ReadObjectContent> {
    header: ObjectHeader,
    file: Box<BufRead>,
    phantom: PhantomData<O>,
}

impl ObjectHandle {
    pub fn read_header(mut file: Box<BufRead>) -> Result<Self> {
        let header = ObjectHeader::read_from(&mut file)?;
        let handle = match header.object_type {
            ObjectType::Blob => {
                ObjectHandle::Blob(RawHandle::new(header, file))
            }
            ObjectType::ChunkedBlob => {
                ObjectHandle::ChunkedBlob(RawHandle::new(header, file))
            }
            ObjectType::Tree => {
                ObjectHandle::Tree(RawHandle::new(header, file))
            }
            ObjectType::Commit => {
                ObjectHandle::Commit(RawHandle::new(header, file))
            }
        };
        Ok(handle)
    }

    pub fn header(&self) -> &ObjectHeader {
        match *self {
            ObjectHandle::Blob(ref raw) => &raw.header,
            ObjectHandle::ChunkedBlob(ref raw) => &raw.header,
            ObjectHandle::Tree(ref raw) => &raw.header,
            ObjectHandle::Commit(ref raw) => &raw.header,
        }
    }

    pub fn parse(self) -> Result<Object> {
        let obj = match self {
            ObjectHandle::Blob(raw) => Object::Blob(raw.parse()?),
            ObjectHandle::ChunkedBlob(raw) => Object::ChunkedBlob(raw.parse()?),
            ObjectHandle::Tree(raw) => Object::Tree(raw.parse()?),
            ObjectHandle::Commit(raw) => Object::Commit(raw.parse()?),
        };
        Ok(obj)
    }
}

impl<O: ReadObjectContent> RawHandle<O> {
    fn new(header: ObjectHeader, file: Box<BufRead>) -> Self {
        RawHandle {
            header: header,
            file: file,
            phantom: PhantomData,
        }
    }
    pub fn parse(mut self) -> Result<O> { O::read_content(&mut self.file) }
}

impl RawHandle<Blob> {
    pub fn copy_content<W: ?Sized + Write>(mut self,
                                           writer: &mut W)
                                           -> Result<()> {
        use std::io::copy;
        let copied = copy(&mut self.file, writer)?;
        assert_eq!(copied, self.header.content_size);
        Ok(())
    }
}
