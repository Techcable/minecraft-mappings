#[cfg(dummy)]
extern crate minecraft_mappings_core as mappings;
extern crate minecraft_mappings_database;

use std::path::PathBuf;

use minecraft_mappings_database::{DatabaseLocation, MappingsDatabase};
use mappings::MinecraftVersion;

const MINECRAFT_VERSION: MinecraftVersion = MinecraftVersion { major: 1, minor: 13, patch: 0 };

fn main() {
    let cache_dir = PathBuf::from("cache");
    let target_dir = PathBuf::from("work/database");
    eprintln!("Creating database in {}, with cache in {}", target_dir.display(), cache_dir.display());
    let location = DatabaseLocation::new(target_dir, cache_dir).unwrap();
    let mut database = MappingsDatabase::open(location).unwrap();
    eprintln!("Writing minecraft obfuscated data for {}", MINECRAFT_VERSION);
    database.create_writer().unwrap()
        .write_obf_data(MINECRAFT_VERSION)
        .unwrap();
}