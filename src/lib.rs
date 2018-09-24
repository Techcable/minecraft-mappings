extern crate indexmap;
extern crate failure;
extern crate failure_derive;
extern crate serde;
extern crate serde_json;
extern crate serde_derive;
extern crate itertools;
extern crate crossbeam;
extern crate parking_lot;
extern crate csv;
extern crate curl;
extern crate scopeguard;
extern crate zip;
extern crate git2;
extern crate srglib;

pub mod mcp;
pub mod spigot;
pub mod cache;
mod version;
mod utils;

pub use self::version::MinecraftVersion;
pub use self::mcp::McpVersion;
