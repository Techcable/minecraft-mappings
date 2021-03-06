use std::iter;
use std::str::FromStr;
use std::io::{copy, BufReader, Read, Cursor};
use std::fs::{self, File};
use std::fmt::{self, Display, Formatter};
use std::path::{PathBuf, Path};
use std::sync::Arc;

use zip::ZipArchive;
use indexmap::{IndexMap};
use failure::Error;
use failure_derive::Fail;
use itertools::PeekingNext;
use serde::ser::{Serialize, Serializer, SerializeStruct};
use serde::de::{self, Deserialize, Deserializer, SeqAccess, MapAccess};
use serde_derive::{Deserialize, Serialize};
use crossbeam::atomic::ArcCell;
use parking_lot::{Mutex};
use srglib::prelude::*;

use crate::utils::LruCache;
use crate::MinecraftVersion;

const MAXIMUM_CACHE_SIZE: usize = 32;
/// The first version where we use the new `mcp-config` system.
///
/// The old system, we fetched SRG data from
/// `"http://files.minecraftforge.net/maven/de/oceanlabs/mcp/mcp/{minecraft_version}/mcp-{minecraft_veresion}-srg.zip"`.
/// These gave us a zip file containing an old-fashioned SRG file that was named `joined.srg`
///
/// Under the new "mcp-config" system, we fetch SRG data from
/// `"http://files.minecraftforge.net/maven/de/oceanlabs/mcp/mcp_config/{minecraft_version}/mcp_config-{minecraft_version}.zip"`.
/// This gives us a zip file where the SRG data is located in `config/joined.tsrg`.
/// It uses the newer and more efficient TSRG format.
const CONFIG_SYSTEM_FIRST_VERSION: MinecraftVersion = MinecraftVersion { major: 1, minor: 13, patch: 0 };

#[derive(Fail, Debug)]
#[fail(display = "Unknown MCP version {:?}", _0)]
pub struct UnknownMcpVersion(McpVersion);

pub(crate) struct McpVersionCache {
    versions: McpVersionList,
    srg_mapping_versions: ArcCell<IndexMap<MinecraftVersion, FrozenMappings>>,
    loaded_versions: ArcCell<LruCache<McpVersion, LoadedVersion>>,
    lock: Mutex<()>,
    cache_location: PathBuf
}
impl McpVersionCache {
    pub fn setup(cache_location: PathBuf) -> Result<McpVersionCache, Error> {
        assert!(cache_location.exists());
        // NOTE: We never cache since we want the latest info
        let versions = McpVersionList::download()?;
        Ok(McpVersionCache {
            versions, srg_mapping_versions: ArcCell::default(),
            loaded_versions: ArcCell::new(Arc::new(LruCache::new(MAXIMUM_CACHE_SIZE))),
            lock: Mutex::new(()),
            cache_location
        })
    }
    pub fn load_srg_mappings(&self, version: MinecraftVersion) -> Result<FrozenMappings, Error> {
        if let Some(srg_mappings) = self.srg_mapping_versions.get().get(&version) {
            return Ok(srg_mappings.clone())
        }
        self.load_srg_mappings_fallback(version)
    }
    #[cold]
    fn load_srg_mappings_fallback(&self, version: MinecraftVersion) -> Result<FrozenMappings, Error> {
        // This ensures we're only loading one mapping at a time
        let _guard = self.lock.lock();
        let srg_mapping_versions = self.srg_mapping_versions.get();
        /*
         * Now that we have the lock,
         * let's check again if our version is present.
         * Someone else could've already loaded it while we were blocking
         */
        if let Some(loaded) = srg_mapping_versions.get(&version) {
            return Ok(loaded.clone());
        }
        let mut updated_srg_mapping_versions = (*srg_mapping_versions).clone();
        drop(srg_mapping_versions); // We're invalidating this
        let version_directory = self.cache_location
            .join(format!("versions/{}", version));
        let mappings_file = version_directory.join("joined-mcp.srg");
        if !mappings_file.exists() {
            if version >= CONFIG_SYSTEM_FIRST_VERSION {
                fs::create_dir_all(&version_directory)?;
                let url = format!(
                    "http://files.minecraftforge.net/maven/de/oceanlabs/mcp/mcp_config/{0}/mcp_config-{0}.zip",
                    version
                );
                let buffer = crate::utils::download_buffer(&url)?;
                let mut archive = ZipArchive::new(Cursor::new(&buffer))?;
                let entry = archive.by_name("config/joined.tsrg")?;
                // For consistency with the old system, we need to translate from TSRG to SRG
                let mappings = TabSrgMappingsFormat::parse_stream(BufReader::new(entry))?;
                let mut file = File::create(&mappings_file)?;
                SrgMappingsFormat::write(&mappings, &mut file)?;
            } else {
                fs::create_dir_all(&version_directory)?;
                let url = format!(
                    "http://files.minecraftforge.net/maven/de/oceanlabs/mcp/mcp/{0}/mcp-{0}-srg.zip",
                    version
                );
                let buffer = crate::utils::download_buffer(&url)?;
                let mut archive = ZipArchive::new(Cursor::new(&buffer))?;
                let mut entry = archive.by_name("joined.srg")?;
                let mut file = File::create(&mappings_file)?;
                copy(&mut entry, &mut file)?;
            }
        }
        let mappings = SrgMappingsFormat::parse_stream(BufReader::new(File::open(&mappings_file)?))?;
        updated_srg_mapping_versions.insert(version, mappings.clone());
        self.srg_mapping_versions.set(Arc::new(updated_srg_mapping_versions));
        Ok(mappings)
    }

    pub fn load_mappings(&self, version: McpVersion) -> Result<Arc<McpMappings>, Error> {
        if let Some(loaded) = self.loaded_versions.get().get(&version) {
            return Ok(loaded.mappings.clone());
        }
        self.load_mappings_fallback(version)
    }
    #[cold]
    fn load_mappings_fallback(&self, version: McpVersion) -> Result<Arc<McpMappings>, Error> {
        let version_info = self.versions.find_version(version)
            .ok_or_else(|| UnknownMcpVersion(version))?;
        // This lock guarantees that only one person will be loading MCP versions at a time
        let _guard = self.lock.lock();
        let loaded_versions = self.loaded_versions.get();
        /*
         * Now that we have the lock,
         * let's check again if our version is present.
         * Someone else could've already loaded it while we were blocking
         */
        if let Some(loaded) = loaded_versions.get(&version) {
            return Ok(loaded.mappings.clone());
        }
        drop(loaded_versions); // We're invalidating this
        let version_directory = self.cache_location
            .join(format!("{}", version.create_spec(true)));
        fs::create_dir_all(&version_directory)?;
        let fields_file = version_directory.join("fields.csv");
        let methods_file = version_directory.join("methods.csv");
        if !fields_file.exists() || !methods_file.exists() {
            version_info.download_into(&fields_file, &methods_file, true)?
        }
        let mut mappings = McpMappings::new();
        mappings.load_fields(&mut ::csv::Reader::from_path(fields_file)?)?;
        mappings.load_methods(&mut ::csv::Reader::from_path(methods_file)?)?;
        let mappings = Arc::new(mappings);

        let mut updated_loaded_versions =
            LruCache::clone(&self.loaded_versions.get());
        updated_loaded_versions.insert(version, LoadedVersion {
            version_info,
            mappings: mappings.clone()
        });
        self.loaded_versions.set(Arc::new(updated_loaded_versions));
        Ok(mappings)
    }
}
#[derive(Clone)]
struct LoadedVersion {
    version_info: McpVersionInfo,
    mappings: Arc<McpMappings>
}
#[derive(Debug)]
pub struct McpMappings {
    pub fields: IndexMap<String, String>,
    pub methods: IndexMap<String, String>
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
            result.map(|entry| (entry.searge, entry.name))
        }).collect()
}
#[derive(Debug, Deserialize)]
struct MappingEntry {
    searge: String,
    name: String,
    side: u32,
    desc: String
}

/// The mcp version info taken from `http://export.mcpbot.bspk.rs/versions.json`
#[derive(Debug, Deserialize)]
struct McpVersionList(IndexMap<MinecraftVersion, ChannelVersionInfo>);
impl McpVersionList {
    pub fn download() -> Result<McpVersionList, Error> {
        let buffer = crate::utils::download_buffer("http://export.mcpbot.bspk.rs/versions.json")?;
        Ok(::serde_json::from_slice(&buffer)?)
    }
    #[inline]
    pub fn find_version(&self, version: McpVersion) -> Option<McpVersionInfo> {
        self.iter().find(|v| v.version == version)
    }
    pub fn iter<'a>(&'a self) -> impl Iterator<Item=(McpVersionInfo)> + 'a {
        self.0.iter().flat_map(|(&minecraft_version, channel_versions)| {
            channel_versions.snapshot.iter()
                    .map(move |&value| McpVersionInfo {
                        minecraft_version, version: McpVersion { value, channel: McpChannel::Snapshot }
                    })
                .chain(channel_versions.stable.iter().map(move |&value| McpVersionInfo {
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
struct McpVersionInfo { // TODO: Rename to ResolvedMcpVersion
    minecraft_version: MinecraftVersion,
    version: McpVersion
}
impl McpVersionInfo {
    fn download_into(&self, fields_file: &Path, methods_file: &Path, nodoc: bool) -> Result<(), Error> {
        let url = self.download_zip_url(nodoc);
        let buffer = crate::utils::download_buffer(&url)?;
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
            channel = self.version.channel,
            docspec = docspec,
            value = self.version.value,
            minecraft_version = self.minecraft_version
        )
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct McpVersion {
    pub value: u32,
    pub channel: McpChannel,
}
impl McpVersion {
    pub fn create_spec(self, nodoc: bool) -> McpVersionSpec {
        McpVersionSpec { version: self, nodoc }
    }
}
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct McpVersionSpec {
    pub version: McpVersion,
    pub nodoc: bool
}
impl McpVersionSpec {
    #[inline]
    pub fn new(value: u32, channel: McpChannel, nodoc: bool) -> McpVersionSpec {
        McpVersionSpec { version: McpVersion { value, channel }, nodoc }
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
        write!(f, "{}", self.version.channel)?;
        if self.nodoc {
            f.write_str("_nodoc")?;
        }
        write!(f, "_{}", self.version.value)
    }
}

impl Serialize for McpVersionSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: Serializer {
        if serializer.is_human_readable() {
            serializer.serialize_str(&format!("{}", self))
        } else {
            let mut s = serializer.serialize_struct("MinecraftVersion", 3)?;
            s.serialize_field("value", &self.version.value)?;
            s.serialize_field("channel", &self.version.channel)?;
            s.serialize_field("nodoc", &self.nodoc)?;
            s.end()
        }
    }
}
impl<'de> Deserialize<'de> for McpVersionSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error> where
        D: Deserializer<'de> {
        struct VersionSpecVisitor;
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field { Value, Channel, Nodoc }
        impl<'de> ::serde::de::Visitor<'de> for VersionSpecVisitor {
            type Value = McpVersionSpec;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("a McpVersionSpec")
            }

            fn visit_str<E>(self, v: &str) -> Result<McpVersionSpec, E> where
                E: de::Error, {
                McpVersionSpec::from_str(v).map_err(de::Error::custom)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<McpVersionSpec, A::Error> where
                A: SeqAccess<'de>, {
                let value = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let channel = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let nodoc = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(2, &self))?;
                Ok(McpVersionSpec::new(value, channel, nodoc))
            }

            fn visit_map<A>(self, mut map: A) -> Result<McpVersionSpec, A::Error> where
                A: MapAccess<'de>, {
                let mut value = None;
                let mut channel = None;
                let mut nodoc = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Value => {
                            if value.is_some() {
                                return Err(de::Error::duplicate_field("major"))
                            }
                            value = Some(map.next_value()?)
                        },
                        Field::Channel => {
                            if channel.is_some() {
                                return Err(de::Error::duplicate_field("minor"))
                            }
                            channel = Some(map.next_value()?)
                        },
                        Field::Nodoc => {
                            if nodoc.is_some() {
                                return Err(de::Error::duplicate_field("patch"))
                            }
                            nodoc = Some(map.next_value()?)
                        },
                    }
                }
                let value = value.ok_or_else(|| de::Error::missing_field("major"))?;
                let channel = channel.ok_or_else(|| de::Error::missing_field("minor"))?;
                // TODO: Should we allow nodoc to be missing to indicate null?
                let nodoc = nodoc.ok_or_else(|| de::Error::missing_field("patch"))?;
                Ok(McpVersionSpec::new(value, channel, nodoc))
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(VersionSpecVisitor)
        } else {
            deserializer.deserialize_struct(
                "McpVersionSpec",
                &["value", "channel", "nodoc"],
                VersionSpecVisitor
            )
        }
    }
}
#[derive(Debug, Fail)]
#[fail(display = "Invalid MCP version spec {:?}", _0)]
pub struct InvalidMcpVersionSpec(String);

#[derive(Debug, Fail)]
#[fail(display = "Invalid MCP channel")]
pub struct InvalidMcpChannel;
