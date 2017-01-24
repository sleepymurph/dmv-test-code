//! Library for master's thesis prototype

#![allow(dead_code)]

#[macro_use]
extern crate error_chain;
extern crate rustc_serialize;

pub mod error {
    //! Error types for the project
    //!
    //! This project uses the [error-chain crate](
    //! https://crates.io/crates/error-chain), and follows its conventions.

    error_chain!{
        foreign_links {
            IoError(::std::io::Error)
                #[doc = "Error caused by an underlying IO error"];
            StripPrefixError(::std::path::StripPrefixError)
                #[doc = "An error during path manipulation"];
        }
        errors {
            PathWithNoParent(p: ::std::path::PathBuf) {
                description("path has no parent")
                display("path has no parent: '{}'", p.display())
            }
            PathWithNoFileName(p: ::std::path::PathBuf) {
                description("path has no file name component")
                display("path has no file name component: '{}'", p.display())
            }
            CorruptCacheFile{
                cache_file: ::std::path::PathBuf,
                cause: ::rustc_serialize::json::DecoderError,
                bad_json: String,
            }
            CacheSerializeError{
                cause: ::rustc_serialize::json::EncoderError,
                bad_cache: ::cache::HashCache,
            }
        }
    }

    pub trait ResultInto<T, E> {
        fn err_into(self) -> Result<T>;
    }

    impl<T, E> ResultInto<T, E> for ::std::result::Result<T, E>
        where E: Into<Error>
    {
        fn err_into(self) -> Result<T> {
            self.map_err(|e| e.into())
        }
    }

    /// Extensions for Paths that work with these custom errors
    pub trait PathExt {
        /// Like `parent()`, but return a Result instead of an Option
        fn parent_or_err(&self) -> Result<&::std::path::Path>;
        /// Like `file_name()`, but return a Result instead of an Option
        fn file_name_or_err(&self) -> Result<&::std::ffi::OsStr>;
    }

    impl PathExt for ::std::path::Path {
        fn parent_or_err(&self) -> Result<&::std::path::Path> {
            self.parent()
                .ok_or_else(|| {
                    ErrorKind::PathWithNoParent(self.to_owned()).into()
                })
        }
        fn file_name_or_err(&self) -> Result<&::std::ffi::OsStr> {
            self.file_name()
                .ok_or_else(|| {
                    ErrorKind::PathWithNoFileName(self.to_owned()).into()
                })
        }
    }
}

#[macro_use]
pub mod wrapperstruct;
pub mod humanreadable;
pub mod encodable;
pub mod constants;
pub mod rollinghash;
pub mod testutil;
pub mod dag;
pub mod objectstore;
pub mod fsutil;
pub mod cache;
pub mod status;
pub mod workdir;
