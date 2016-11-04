use std::io;
use std::fs;
use std::path;
use std::ops;
use super::*;
use dag::*;

pub struct DiskRepository {
    path: path::PathBuf,
}

pub struct DiskIncoming {
    path: path::PathBuf,
    file: fs::File,
}

impl DiskRepository {
    pub fn new(path: &path::Path) -> Self {
        DiskRepository { path: path.to_owned() }
    }

    fn path(&self) -> &path::PathBuf {
        &self.path
    }

    fn object_path(&self, key: &ObjectKey) -> path::PathBuf {
        self.path
            .join("objects")
            .join(&key[0..2])
            .join(&key[2..4])
            .join(&key[4..])
    }
}

impl Repository for DiskRepository {
    type IncomingType = DiskIncoming;

    fn init(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.path)
    }

    fn has_object(&mut self, key: &ObjectKey) -> bool {
        self.object_path(key).is_file()
    }
    fn stat_object(&mut self, key: &ObjectKey) -> ObjectStat {
        unimplemented!();
    }
    fn read_object(&mut self, key: &ObjectKey) -> &mut io::Read {
        unimplemented!();
    }
    fn add_object(&mut self) -> io::Result<DiskIncoming> {
        DiskIncoming::new(&self.path.join("tmp"))
    }
}

impl DiskIncoming {
    fn new(path: &path::Path) -> io::Result<Self> {
        let file = try!(fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&path));
        Ok(DiskIncoming {
            path: path.to_owned(),
            file: file,
        })
    }
}

impl IncomingObject for DiskIncoming {
    fn set_key(self, _key: &ObjectKey) -> io::Result<()> {
        Ok(())
    }
}

impl io::Write for DiskIncoming {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

mod test {
    extern crate tempdir;

    use std::path;
    use std::io;
    use std::io::Write;
    use std::fs;
    use std::ffi;
    use super::*;
    use super::super::*;

    fn mem_temp_repo() -> (tempdir::TempDir, DiskRepository) {
        let tempdir = tempdir::TempDir::new_in("/dev/shm/", "rust_test")
            .expect("could not create temporary directory in /dev/shm/");

        let mut repo = DiskRepository::new(&tempdir.path().join("repo"));
        repo.init().expect("could not initialize temporary repo");

        assert_eq!(repo.path().file_name().unwrap(), "repo");
        assert_eq!(repo.path().is_dir(), true);

        (tempdir, repo)
    }

    #[test]
    fn test_object_path() {
        let mut repo = DiskRepository::new(path::Path::new(".prototype"));
        assert_eq!(
            repo.object_path("a9c3334cfee4083a36bf1f9d952539806fff50e2"),
            path::Path::new(".prototype/objects/")
                        .join("a9/c3/334cfee4083a36bf1f9d952539806fff50e2"));
    }

    #[test]
    fn test_add_object() {
        let (dir, mut repo) = mem_temp_repo();
        let mut incoming = repo.add_object().expect("could not open incoming");
        incoming.write(b"here be content")
            .expect("could not write to incoming");
        incoming.flush().expect("could not flush incoming");
        let key = "9cac8e6ad1da3212c89b73fdbb2302180123b9ca";
        incoming.set_key(key)
            .expect("could not set key");
        // assert_eq!(repo.has_object(key), true);
    }
}
