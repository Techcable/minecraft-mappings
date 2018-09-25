//! Any combination of the following mapping systems are supported for any given minecraft version:
//! - `srg` - MCP's unique srg mappings, which are the same for each minecraft version.
//! - `mcp` - MCP's crowd sourced deobfuscated mappings, fetched from `MCPBot`
//!   - These have a independent version based on the date, which must be externally specified.
//! - `spigot` - Spigot's deobfuscation mappings, held in the `BuildData` git repo
//!   - These are significantly lower quality than the MCP mappings, and most member names are still obfuscated
//!   - These mappings don't change very often, since plugins use them and would break if they changed
//! - `obf` - The obfuscated mojang names, which are internally used to unify the different mappings systems
//!
//! Mapping targets have a string representation of the form `{original}2{renamed}-{flags}-{minecraft_version}` with an optional modifier at the end.
//! For example, `spigot2mcp` specifies mappings from the spigot names into the MCP names.
//! Three modifiers are supported:
//! - `classes` - Restricts the mappings to just class names.
//! - `members` - Restricts the mappings to just member names.
//! - `onlyobf` - Restricts the mappings to just names that are still obfuscated.
//!   - This allows you to take advantage of other mappings,
//!     without changing names that are already deobfuscated.
//!   - The motivating example is `spigot2mcp-onlyobf`,
//!     which would take advantage of the MCP mappings
//!     without changing names spigot already deobfuscated.
//!   - I know I have personally become familiar with the spigot naming scheme
//!      but I still want to take advantage of MCP naming information where spigot is lacking.
#![feature(min_const_fn)]
extern crate srglib;
#[cfg(dummy)]
extern crate minecraft_mappings_core as mappings;
extern crate bitflags;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate indexmap;

mod target;
mod computer;

pub use self::target::{TargetMapping, MappingSystem};
pub use self::computer::MappingsTargetComputer;
