#![feature(plugin)]
#![plugin(rocket_codegen)]
extern crate indexmap;
extern crate rocket;
extern crate rocket_contrib;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
#[cfg(dummy)] // TODO: Drop when IntelliJ gets support
extern crate minecraft_mappings_core as mappings;
#[cfg(dummy)]
extern crate minecraft_mappings_engine as engine;
extern crate srglib;
extern crate failure;

use std::path::PathBuf;
use std::time::{Instant, Duration};

use failure::Error;
use rocket::State;
use indexmap::IndexMap;
use serde_derive::{Deserialize, Serialize};
use rocket_contrib::Json;
use engine::{TargetMapping, MappingsTargetComputer};
use mappings::{McpVersionSpec, MinecraftVersion, cache::MinecraftMappingsCache};
use srglib::prelude::*;

#[derive(Debug, Deserialize)]
struct MappingsRequest {
    minecraft_version: MinecraftVersion,
    #[serde(default)]
    mcp_version: Option<McpVersionSpec>,
    targets: Vec<TargetMapping>
}
#[derive(Debug, Serialize)]
struct MappingsResponse {
    serialized_mappings: IndexMap<TargetMapping, String>,
    /// The total resposne time in milliseconds
    response_time: u64
}

#[post("/api/beta/load_mappings", format = "application/json", data = "<request>")]
fn load_mappings(cache: State<MinecraftMappingsCache>, request: Json<MappingsRequest>) -> Result<Json<MappingsResponse>, Error> {
    let start = Instant::now();
    let request: &MappingsRequest = &request.0; // TODO: IntelliJ can't handle the defualt type paramter
    let computer = MappingsTargetComputer::new(
        &cache,
        request.minecraft_version,
        request.mcp_version.map(|version| version.version)
    );
    let mut serialized_mappings =
        IndexMap::with_capacity(request.targets.len());
    for &target in &request.targets {
        let mappings = computer.compute_target(target)?;
        let serialized = SrgMappingsFormat::write_string(&mappings);
        serialized_mappings.insert(target, serialized);
    }
    let response_time = to_millis(start.elapsed());
    Ok(Json(MappingsResponse { serialized_mappings, response_time }))
}
fn to_millis(d: Duration) -> u64 {
    d.as_secs().saturating_mul(1000)
        .saturating_add(d.subsec_millis() as u64)
}

fn main() {
    let cache = MinecraftMappingsCache::setup(PathBuf::from("cache"))
        .expect("Unable to setup cache");
    rocket::ignite()
        .manage(cache)
        .mount("/", routes![load_mappings])
        .launch();
}