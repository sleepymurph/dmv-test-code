//! Library for master's thesis prototype

#![allow(dead_code)]

extern crate rustc_serialize;

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
