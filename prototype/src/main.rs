#[macro_use]
extern crate clap;
extern crate prototypelib;
#[macro_use]
extern crate error_chain;

use prototypelib::cmd;
use prototypelib::error::*;
use std::env;
use std::path;

// Have error_chain create a main() function that handles Results
quick_main!(run);

fn run() -> Result<()> {

    let arg_yaml = load_yaml!("cli.yaml");
    let argmatch = clap::App::from_yaml(arg_yaml).get_matches();

    match argmatch.subcommand_name() {
        Some(name) => {
            // Match on subcommand and delegate to a subcommand handler function
            let subfn = match name {
                "init" => cmd_init,
                "hash-object" => cmd_hash_object,
                "show-object" => cmd_show_object,
                "cache-status" => cmd_cache_status,
                _ => unimplemented!(),
            };
            let submatch = argmatch.subcommand_matches(name)
                .expect("just matched");
            subfn(&argmatch, submatch)
        }
        None => unimplemented!(),
    }
}

fn cmd_init(_argmatch: &clap::ArgMatches,
            _submatch: &clap::ArgMatches)
            -> Result<()> {
    let repo_path = env::current_dir().expect("current dir");

    cmd::init(repo_path)
}

fn cmd_hash_object(_argmatch: &clap::ArgMatches,
                   submatch: &clap::ArgMatches)
                   -> Result<()> {
    let repo_path = env::current_dir().expect("current dir");

    let file_path = submatch.value_of("filepath").expect("required");
    let file_path = path::PathBuf::from(file_path);

    cmd::hash_object(repo_path, file_path)
}

fn cmd_show_object(_argmatch: &clap::ArgMatches,
                   submatch: &clap::ArgMatches)
                   -> Result<()> {
    let repo_path = env::current_dir().expect("current dir");
    let hash = submatch.value_of("hash").expect("required");

    cmd::show_object(repo_path, hash)
}

fn cmd_cache_status(_argmatch: &clap::ArgMatches,
                    submatch: &clap::ArgMatches)
                    -> Result<()> {
    let file_path = submatch.value_of("filepath").expect("required");
    let file_path = path::PathBuf::from(file_path);

    cmd::cache_status(file_path)
}
