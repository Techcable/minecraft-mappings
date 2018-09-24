use std::iter;
use std::str::FromStr;
use std::io::{copy, Read, Cursor};
use std::fs::File;
use std::fmt::{self, Display, Formatter};
use std::cell::RefCell;
use std::path::{PathBuf, Path};
use std::sync::Arc;
use std::time::Duration;

use zip::ZipArchive;
use indexmap::{IndexMap, IndexSet};
use failure::Error;
use failure_derive::Fail;
use itertools::PeekingNext;
use serde::{Deserialize, Deserializer};
use crossbeam::atomic::ArcCell;
use parking_lot::{Mutex, Condvar};

use crate::utils::LruCache;
use crate::MinecraftVersion;

const MAXIMUM_CACHE_SIZE: usize = 32;
const MAPPINGS_WAIT_DURATION: Duration = Duration::from_millis(500);


#[derive(Fail, Debug)]
#[fail(display = "Unknown MCP version {:?}")]
pub struct UnknownMcpVersion(McpVersion);

pub struct McpVersionCache {
    versions: McpVersionList,
    loaded_versions: ArcCell<LruCache<McpVersion, LoadedVersion>>,
    lock: Mutex<()>,
    cache_location: PathBuf
}
impl McpVersionCache {
    pub fn load_mappings(&self, version: McpVersion) -> Result<Arc<McpMappings>, Error> {
        let loaded_versions =
            self.loaded_versions.get();
        if let Some(loaded) = loaded_versions.get(&version) {
            return Ok(loaded.mappings.clone());
        }
        self.load_mappings_fallback(version)
    }
    #[cold]
    fn load_mappings_fallback(&self, version: McpVersion) -> Result<Arc<McpMappings>, Error> {
        let version_info = self.versions.find_version(version)
            .ok_or_else(|| UnknownMcpVersion(version))?;
        // This lock guarantees that only one person will be loading MCP versions at a time
        let guard = self.lock.lock();
        let loaded_versions = self.loaded_versions.get();
        /*
         * Now that we have the lock,
         * let's check again if our version is present.
         * Someone else could've already loaded it while we were blocking
         */
        if let Some(loaded) = loaded_versions.get(&version) {
            return Ok(loaded.mappings.clone());
        }
        let mut updated_loaded_versions =
            (*loaded_versions).clone();
        drop(loaded_versions); // We're invalidating this
        let version_directory = self.cache_location
            .join(format!("{}", version.create_spec(true)));
        let fields_file = version_directory.join("fields.csv");
        let methods_file = version_directory.join("methods.csv");
        if !fields_file.exists() || !methods_file.exists() {
            version.download_into(&fields_file, &methods_file, true)?
        }
        let mut mappings = McpMappings::new();
        mappings.load_fields(&mut ::csv::Reader::from_path(fields_file)?)?;
        mappings.load_methods(&mut ::csv::Reader::from_path(methods_file)?)?;
        let mappings = Arc::new(mappings);
        updated_loaded_versions.insert(version, LoadedVersion {
            version_info,
            mappings: mappings.clone()
        });
        Ok(mappings)
    }
}
struct LoadedVersion {
    version_info: McpVersionInfo,
    mappings: Arc<McpMappings>
}
#[derive(Debug)]
pub struct McpMappings {
    fields: IndexMap<String, String>,
    methods: IndexMap<String, String>
}
impl McpMappings {
    #[inline]
    pub fn new() -> Self {
        McpMappings {
            fields: IndexMap::new(),
            methods: IndexMap::new()
        }
    }
    fn load_fields<R: Read>(&mut self, reader: &mut ::csv::Reader<R>) -> Result<(), ::csv::Error> {
        self.fields = load_record_map(reader)?;
        Ok(())
    }
    fn load_methods<R: Read>(&mut self, reader: &mut ::csv::Reader<R>) -> Result<(), ::csv::Error> {
        self.methods = load_record_map(reader)?;
        Ok(())
    }
}
fn load_record_map<R: Read>(
    reader: &mut ::csv::Reader<R>
) -> Result<IndexMap<String, String>, ::csv::Error> {
    reader.deserialize::<MappingEntry>()
        .map(|result| {
            result.map(|entry| (entry.serage, entry.name))
        }).collect()
}
#[derive(Debug, Deserialize)]
struct MappingEntry {
    serage: String,
    name: String,
    side: u32,
    desc: String
}

/// The mcp version info taken from `http://export.mcpbot.bspk.rs/versions.json`
#[derive(Debug, Deserialize)]
pub struct McpVersionList(IndexMap<MinecraftVersion, ChannelVersionInfo>);
impl McpVersionList {
    #[inline]
    pub fn find_version(&self, version: McpVersion) -> Option<McpVersionInfo> {
        self.iter().find(|v| v.version == version)
    }
    pub fn iter<'a>(&'a self) -> impl Iterator<Item=(McpVersionInfo)> + 'a {
        self.0.iter().flat_map(|(&minecraft_version, channel_versions)| {
            channel_versions.snapshot.iter()
                    .map(|&value| McpVersionInfo {
                        minecraft_version, version: McpVersion { value, channel: McpChannel::Snapshot }
                    })
                .chain(channel_versions.stable.iter().map(|&value| McpVersionInfo {
                    minecraft_version, version: McpVersion { value, channel: McpChannel::Stable }
                }))
        })
    }
}
#[derive(Deserialize, Debug)]
struct ChannelVersionInfo {
    snapshot: Vec<u32>,
    stable: Vec<u32>
}

#[derive(Copy, Clone, Debug)]
pub enum McpChannel {
    Snapshot,
    Stable
}

impl FromStr for McpChannel {
    type Err = InvalidMcpChannel;

    fn from_str(s: &str) -> Result<Self, InvalidMcpChannel> {
        Ok(match s {
            "snapshot" => McpChannel::Snapshot,
            "stable" => McpChannel::Stable,
            _ => return Err(InvalidMcpChannel)
        })
    }
}
impl Display for McpChannel {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(match *self {
            McpChannel::Snapshot => "snapshot",
            McpChannel::Stable => "Stable",
        })
    }
}

#[derive(Copy, Clone, Debug)]
pub struct McpVersionInfo { // TODO: Rename to ResolvedMcpVersion
    minecraft_version: MinecraftVersion,
    version: McpVersion
}
impl McpVersionInfo {
    fn download_into(&self, fields_file: &Path, methods_file: &Path, nodoc: bool) -> Result<(), Error> {
        let url = self.download_zip_url(nodoc);
        let buffer = ::utils::download_buffer(&url)?;
        let mut archive = ZipArchive::new(Cursor::new(&buffer))?;
        let mut fields_file = File::create(fields_file)?;
        let mut methods_file = File::create(methods_file)?;
        copy(&mut archive.by_name("fields.csv")?, &mut fields_file)?;
        copy(&mut archive.by_name("methods.csv")?, &mut methods_file)?;
        Ok(())
    }
    fn download_zip_url(&self, nodoc: bool) -> String {
        let docspec = if nodoc { "_nodoc" } else { "" };
        format!(
            "http://export.mcpbot.bspk.rs/mcp_{channel}{docspec}/\
            {value}-{minecraft_version}/mcp_{channel}{docspec}-{value}-{minecraft_version}.zip",
            channel = self.channel,
            docspec = docspec,
            value = self.value,
            minecraft_version = self.minecraft_version
        )
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct McpVersion {
    value: u32,
    channel: McpChannel,
}
impl McpVersion {
    pub fn create_spec(self, nodoc: bool) -> McpVersionSpec {
        McpVersionSpec { version: self, nodoc }
    }
}
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct McpVersionSpec {
    version: McpVersion,
    nodoc: bool
}
impl McpVersionSpec {
    #[inline]
    pub(crate) fn forbid_docs(self) {
        assert!(self.nodoc, "Docs are forbidden: {}", self);
    }
    #[inline]
    pub fn without_docs(mut self) -> McpVersionSpec {
        self.nodoc = true;
        self
    }
}
impl FromStr for McpVersionSpec {
    type Err = InvalidMcpVersionSpec;

    fn from_str(s: &str) -> Result<Self, InvalidMcpVersionSpec> {
        let mut iter: iter::Peekable<_> = s.split('_').peekable();
        let error = || InvalidMcpVersionSpec(s.into());
        let channel = iter.next().ok_or_else(error)?
            .parse::<McpChannel>().map_err(|_| error())?;
        let nodoc = iter.peeking_next(|item| *item == "nodoc")
            .is_some();
        let value = iter.next().ok_or_else(error)?
            .parse::<u32>().map_err(|_| error())?;
        Ok(McpVersionSpec { version: McpVersion { value, channel }, nodoc })
    }
}
impl Display for McpVersionSpec {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!("{}", self.channel)?;
        if self.nodoc {
            f.write_str("_nodoc")?;
        }
        write!("_{}", self.value)
    }
}
#[derive(Debug, Fail)]
#[fail(display = "Invalid MCP version sepc {:?}", _0)]
pub struct InvalidMcpVersionSpec(String);

#[derive(Debug, Fail)]
#[fail(display = "Invalid MCP channel")]
pub struct InvalidMcpChannel;
