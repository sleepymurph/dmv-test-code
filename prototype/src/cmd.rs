//! High-level commands

use cache;
use dag;
use dag::ObjectCommon;
use error::*;
use objectstore;
use pipeline;
use std::io;
use std::path;

pub fn init(repo_path: path::PathBuf) -> Result<()> {
    try!(objectstore::ObjectStore::init(repo_path));
    Ok(())
}

pub fn hash_object(repo_path: path::PathBuf,
                   file_path: path::PathBuf)
                   -> Result<()> {

    let mut hash_setup = try!(pipeline::HashSetup::with_repo_path(repo_path));

    let hash;
    if file_path.is_file() {
        hash = try!(hash_setup.hash_file(file_path.clone()));
    } else if file_path.is_dir() {
        unimplemented!()
    } else {
        unimplemented!()
    };

    println!("{} {}", hash, file_path.display());
    Ok(())
}

pub fn show_object(repo_path: path::PathBuf, hash: &str) -> Result<()> {

    let hash = try!(dag::ObjectKey::from_hex(hash));
    let objectstore = try!(objectstore::ObjectStore::open(repo_path));

    if !objectstore.has_object(&hash) {
        println!("No such object");
    } else {
        let reader = try!(objectstore.open_object_file(&hash));
        let mut reader = io::BufReader::new(reader);

        let header = try!(dag::ObjectHeader::read_from(&mut reader));
        match header.object_type {
            dag::ObjectType::Blob => {
                println!("{}", header);
            }
            _ => {
                let object = try!(header.read_content(&mut reader));
                println!("{}", object.pretty_print());
            }
        }
    }
    Ok(())
}

pub fn cache_status(file_path: path::PathBuf) -> Result<()> {
    let mut cache = cache::AllCaches::new();
    let cache_status = try!(cache.check(&file_path));
    println!("{:?}", cache_status);
    Ok(())
}
