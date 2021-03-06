#[cfg(dummy)] // Needed for IntelliJ autocomplete
extern crate minecraft_mappings_core as mappings;
#[cfg(dummy)]
extern crate minecraft_mappings_engine as engine;
#[macro_use]
extern crate clap;

use std::path::PathBuf;
use std::io::BufWriter;
use std::fs::{self, File};
use std::time::{Duration, Instant};

use failure::Error;
use srglib::prelude::*;

use mappings::cache::MinecraftMappingsCache;
use mappings::{McpVersion, McpVersionSpec, MinecraftVersion};
use engine::{TargetMapping, MappingsTargetComputer};

fn app() -> clap::App<'static, 'static> {
    clap_app!(minecraft_mappings =>
        (version: crate_version!())
        (author: crate_authors!())
        (about: crate_description!())
        (@arg output_dir: --out +takes_value default_value[out] "The output directory to place mappings")
        (@arg mcp_version: --mcp +takes_value "The MCP version to generate mappings for")
        (@arg cache: --cache +takes_value default_value[cache] "Specify an alternate cache location")
        (@arg minecraft_version: +required "The minecraft version to generate the mappings for")
        (@arg targets: +required +multiple "The target mappings to generate")
    )
}

fn main() -> Result<(), Error> {
    let matches = app().get_matches();
    let targets: Vec<TargetMapping> = values_t!(matches, "targets", TargetMapping)
        .unwrap_or_else(|e| e.exit());
    let minecraft_version = value_t!(matches, "minecraft_version", MinecraftVersion)
        .unwrap_or_else(|e| e.exit());
    // Demand a MCP version (if needed)
    let needs_mcp_version = targets.iter()
        .any(TargetMapping::needs_mcp_version);
    let mcp_version: Option<McpVersion> = if needs_mcp_version {
        Some(value_t!(matches, "mcp_version", McpVersionSpec)
            .unwrap_or_else(|e| e.exit()).version)
    } else {
        None
    };
    let cache_location = PathBuf::from(matches.value_of("cache").unwrap());
    let out = PathBuf::from(matches.value_of("output_dir").unwrap());
    fs::create_dir_all(&cache_location)?;
    fs::create_dir_all(&out)?;
    let cache = MinecraftMappingsCache::setup(cache_location.clone())?;
    let start = Instant::now();
    let computer = MappingsTargetComputer::new(&cache, minecraft_version, mcp_version);
    for &target in &targets {
        let out_location = out.join(format!("{}.srg", target));
        let target_start = Instant::now();
        let mappings = computer.compute_target(target)?;
        let writer = BufWriter::new(File::create(out_location)?);
        SrgMappingsFormat::write(&mappings, writer)?;
        println!("  Finished {} in {}ms", target, duration_to_millis(target_start.elapsed()));
    }
    println!("Finished {} targets in {}ms", targets.len(), duration_to_millis(start.elapsed()));
    Ok(())
}
fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_secs().saturating_mul(1000)
        .saturating_add(duration.subsec_millis().into())
}