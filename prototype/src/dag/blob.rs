

use std::io;
use std::io::Write;
use super::*;

/// A binary (file) stored in the DAG
///
/// Blobs are assumed to be able to fit in memory because of the way that
/// large files are broken into chunks when stored. So it should be safe to
/// use a `Vec<u8>` to hold the contents.
#[derive(Clone,Eq,PartialEq,Hash,Debug)]
pub struct Blob {
    pub content: Vec<u8>,
}

impl From<Vec<u8>> for Blob {
    fn from(v: Vec<u8>) -> Blob {
        Blob::from_vec(v)
    }
}

impl Blob {
    pub fn from_vec(v: Vec<u8>) -> Blob {
        Blob { content: v }
    }

    pub fn size(&self) -> ObjectSize {
        self.content.len() as ObjectSize
    }

    pub fn content(&self) -> &Vec<u8> {
        &self.content
    }
}

impl Object for Blob {
    fn write_to<W: io::Write>(&self, writer: &mut W) -> io::Result<ObjectKey> {
        let mut writer = HashWriter::wrap(writer);
        let header = ObjectHeader {
            object_type: ObjectType::Blob,
            content_size: self.content.len() as ObjectSize,
        };
        try!(header.write_to(&mut writer));
        try!(writer.write(&self.content));
        Ok(writer.hash())
    }
    fn read_from<R: io::BufRead>(reader: &mut R) -> Result<Self, DagError> {
        let mut content: Vec<u8> = Vec::new();
        try!(reader.read_to_end(&mut content));
        Ok(Blob { content: content })
    }
}


#[cfg(test)]
mod test {

    use std::io;
    use super::super::*;

    #[test]
    fn test_write_blob() {
        // Construct object
        let content = b"Hello world!";
        let content_size = content.len() as ObjectSize;
        let blob = Blob::from_vec(content.to_vec());

        // Write out
        let mut output: Vec<u8> = Vec::new();
        blob.write_to(&mut output).expect("write out blob");

        // Uncomment to double-check format
        // panic!(format!("{:?}",output));

        // Read in header
        let mut reader = io::BufReader::new(output.as_slice());
        let header = ObjectHeader::read_from(&mut reader).expect("read header");

        assert_eq!(header,
                   ObjectHeader {
                       object_type: ObjectType::Blob,
                       content_size: content_size,
                   });

        // Read in object content
        let readblob = Blob::read_from(&mut reader).expect("read rest of blob");

        assert_eq!(readblob,
                   blob,
                   "Should be able to get the rest of the content by \
                    continuing to read from the same reader.");
    }
}