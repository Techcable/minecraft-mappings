#[cfg(dummy)]
extern crate minecraft_mappings_core as mappings;

use std::path::PathBuf;
use std::env;
use std::process::exit;

use minecraft_mappings_database::{DatabaseLocation, MappingsDatabase};
use mappings::MinecraftVersion;

const MINECRAFT_VERSION: MinecraftVersion = MinecraftVersion { major: 1, minor: 13, patch: 0 };

fn main() {
    // TODO: Redo all this with clap
    ::env_logger::init();
    let cache_dir = PathBuf::from("cache");
    let target_dir = PathBuf::from("work/database");
    eprintln!("Creating database in {}, with cache in {}", target_dir.display(), cache_dir.display());
    let location = DatabaseLocation::new(target_dir, cache_dir).unwrap();
    let mut database = MappingsDatabase::open(location).unwrap();
    let args: Vec<String> = env::args().skip(1).collect();
    match args.get(0).map(String::as_str) {
        None => {
            eprintln!("Missing command");
            exit(1);
        },
        Some("load-test") => {
            eprintln!("Loading data for minecraft version {}", MINECRAFT_VERSION);
            database.write_initial_data(MINECRAFT_VERSION).unwrap()
        },
        Some(command) => {
            eprintln!("Unknown command {:?}", command);
            exit(1);
        }
    }
}