use std::path::PathBuf;
use std::fs;
use std::sync::Arc;

use failure::Error;
use srglib::prelude::*;

use crate::MinecraftVersion;
use crate::spigot::{SpigotMappingsCache, SpigotMappings};
use crate::mcp::{McpVersionCache, McpMappings, McpVersion};

pub struct MinecraftMappingsCache {
    spigot: SpigotMappingsCache,
    mcp: McpVersionCache
}
impl MinecraftMappingsCache {
    pub fn setup(location: PathBuf) -> Result<MinecraftMappingsCache, Error> {
        fs::create_dir_all(&location)?;
        let mcp_cache = location.join("mcp");
        let spigot_cache = location.join("spigot");
        fs::create_dir_all(&mcp_cache)?;
        fs::create_dir_all(&spigot_cache)?;
        let spigot = SpigotMappingsCache::setup(spigot_cache)?;
        let mcp = McpVersionCache::setup(mcp_cache)?;
        Ok(MinecraftMappingsCache { spigot, mcp })
    }
    #[inline]
    pub fn load_mcp_mappings(&self, mcp: McpVersion) -> Result<Arc<McpMappings>, Error> {
        self.mcp.load_mappings(mcp)
    }
    #[inline]
    pub fn load_srg_mappings(&self, version: MinecraftVersion) -> Result<FrozenMappings, Error> {
        self.mcp.load_srg_mappings(version)
    }
    #[inline]
    pub fn load_spigot_mappings(&self, version: MinecraftVersion) -> Result<Arc<SpigotMappings>, Error> {
        self.spigot.load_mappings(version)
    }
}