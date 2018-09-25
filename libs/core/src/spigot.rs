use std::io::{Read, BufReader, Write, Cursor};
use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::sync::Arc;

use indexmap::IndexMap;
use failure::Error;
use git2::{Repository, Commit, Oid};
use srglib::prelude::*;
use crossbeam::atomic::ArcCell;
use parking_lot::Mutex;
use serde_derive::Deserialize;

use crate::MinecraftVersion;
use crate::utils::load_from_commit;

fn transform_spigot_packages(s: &str) -> Option<String> {
    if s.is_empty() { Some("net/minecraft/server".into()) } else { None }
}

pub(crate) struct SpigotMappingsCache {
    cache_location: PathBuf,
    // NOTE: Since spigot has significantly fewer versions, we don't need have LRU eviction
    versions: ArcCell<IndexMap<MinecraftVersion, Arc<SpigotMappings>>>,
    lock: Mutex<()>,
}
impl SpigotMappingsCache {
    pub fn setup(cache_location: PathBuf) -> Result<SpigotMappingsCache, Error> {
        assert!(cache_location.exists());
        Ok(SpigotMappingsCache { cache_location, versions: ArcCell::default(), lock: Mutex::new(()) })
    }
    pub fn load_mappings(&self, version: MinecraftVersion) -> Result<Arc<SpigotMappings>, Error> {
        if let Some(loaded) = self.versions.get().get(&version) {
            return Ok(loaded.clone());
        }
        self.load_mappings_fallback(version)
    }
    #[cold]
    fn load_mappings_fallback(&self, version: MinecraftVersion) -> Result<Arc<SpigotMappings>, Error> {
        // This lock guarantees that only one person will be loading versions at a time
        let _guard = self.lock.lock();
        let versions = self.versions.get();
        /*
         * Now that we have the lock,
         * let's check again to se if our version is present.
         * Someone else could've already loaded it while we were blocking
         */
        if let Some(loaded) = versions.get(&version) {
            return Ok(loaded.clone());
        }
        let info = self.load_version_info(version)?;
        let mut updated_versions =
            (*versions).clone();
        drop(versions); // We're invalidating this
        let version_directory = self.cache_location
            .join(format!("versions/{}", version));
        fs::create_dir_all(&version_directory)?;
        let class_file = version_directory.join("class.srg");
        let members_file = version_directory.join("members.srg");
        let combined_file = version_directory.join("chained.srg");
        if !class_file.exists() || !members_file.exists() || !combined_file.exists() {
            let build_data = self.fetch_build_data(&info.refs.build_data)?;
            let oid = Oid::from_str(&info.refs.build_data)?;
            let commit = build_data.find_commit(oid)?;
            let class_mappings = commit.read_class_mappings()?;
            let member_mappings = commit.read_member_mappings()?;
            let chained = class_mappings.clone().chain(member_mappings.clone())
                .transform_packages(transform_spigot_packages);
            SrgMappingsFormat::write(&class_mappings, File::create(&class_file)?)?;
            SrgMappingsFormat::write(&member_mappings, File::create(&members_file)?)?;
            SrgMappingsFormat::write(&chained, File::create(&combined_file)?)?;
        }
        let class_mappings = SrgMappingsFormat::parse_stream(BufReader::new(File::open(&class_file)?))?;
        let member_mappings = SrgMappingsFormat::parse_stream(BufReader::new(File::open(&members_file)?))?;
        let chained_mappings = SrgMappingsFormat::parse_stream(BufReader::new(File::open(&combined_file)?))?;
        let mappings = Arc::new(SpigotMappings { class_mappings, member_mappings, chained_mappings });
        updated_versions.insert(version, mappings.clone());
        self.versions.set(Arc::new(updated_versions));
        Ok(mappings)
    }
    fn load_version_info(&self, version: MinecraftVersion) -> Result<VersionInfo, Error> {
        let location = self.cache_location
            .join(format!("version_info/{}.json", version));
        fs::create_dir_all(location.parent().unwrap())?;
        if !location.exists() {
            // If we don't have it locally we need to check spigot
            let url = format!("https://hub.spigotmc.org/versions/{}.json", version);
            let buffer = match crate::utils::download_buffer(&url) {
                Err(ref e) if e.downcast_ref::<crate::utils::HttpNotFound>().is_some() => {
                    // If it's a 404, then we know it's an unknown version
                    return Err(version.unknown().into())
                },
                Err(e) => return Err(e),
                Ok(buffer) => buffer
            };
            let mut file = File::create(&location)?;
            file.write_all(&buffer)?;
            drop(file);
        }
        Ok(::serde_json::from_reader(File::open(&location)?)?)
    }
    /// Fetch spigot BuildData and ensure it contains the specified commit
    fn fetch_build_data(&self, commit: &str) -> Result<BuildData, Error> {
        let repo_location = self.cache_location.join("BuildData");
        fs::create_dir_all(repo_location.parent().unwrap())?;
        let repo_url = "https://hub.spigotmc.org/stash/scm/spigot/builddata.git";
        let commit_id = Oid::from_str(commit)?;
        let repo = if !repo_location.exists() {
            println!("Fetching BuildData@{}", commit);
            Repository::clone(repo_url, repo_location)?
        } else {
            let repo = Repository::open(repo_location)?;
            if repo.find_commit(commit_id).is_err() {
                println!("Updating BuildData@{}", commit);
                // Update the repo if we don't have the commit we want
                let mut remote = repo.remote_anonymous(repo_url)?;
                remote.fetch(
                    &["master", format!(":{}", commit).as_ref()],
                    None,
                    None,
                )?;
            }
            repo
        };
        Ok(BuildData(repo))
    }
}
/// Contains all the mappings for a specific version
pub struct SpigotMappings {
    pub class_mappings: FrozenMappings,
    pub member_mappings: FrozenMappings,
    pub chained_mappings: FrozenMappings
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionInfoRefs {
    pub build_data: String,
    pub bukkit: String,
    pub craft_bukkit: String,
    pub spigot: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionInfo {
    pub name: String,
    pub refs: VersionInfoRefs,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildDataInfo {
    pub minecraft_version: String,
    pub minecraft_hash: String,
    pub access_transforms: String,
    pub class_mappings: String,
    pub member_mappings: String,
    pub package_mappings: String,
}
impl BuildDataInfo {
    #[inline]
    pub fn read<R: Read>(input: &mut R) -> Result<BuildDataInfo, Error> {
        Ok(::serde_json::from_reader(input)?)
    }
}
struct BuildData(pub Repository);
impl BuildData {
    pub fn find_commit(&self, id: Oid) -> Result<BuildDataCommit, Error> {
        let commit = self.0.find_commit(id)?;
        let mut build_data_buffer = String::new();
        load_from_commit(
            &self.0,
            &commit,
            Path::new("info.json"),
            &mut build_data_buffer,
        )?;
        let info = BuildDataInfo::read(&mut Cursor::new(build_data_buffer))?;
        Ok(BuildDataCommit {
            info,
            commit,
            data: self,
        })
    }
}
fn sanitize_class_data(s: &mut String) {
    let mut corrected = String::with_capacity(s.len());
    let mut first = true;
    for line in s.lines() {
        /*
         * We're trying to strip the invalid lines in the 1.8.8 data which contain dots.
         * Since only invalid lines (and comments) contain dots, we can just blindly remove them.
         */
        if !line.contains('.') {
            if !first {
                corrected.push('\n');
            }
            first = false;
            corrected.push_str(line);
        }
    }
    *s = corrected;
}
struct BuildDataCommit<'a> {
    info: BuildDataInfo,
    commit: Commit<'a>,
    data: &'a BuildData,
}
impl<'a> BuildDataCommit<'a> {
    #[inline]
    fn load(&self, path: &Path, buffer: &mut String) -> Result<(), Error> {
        load_from_commit(&self.data.0, &self.commit, path, buffer)?;
        Ok(())
    }
    pub fn read_class_mappings(&self) -> Result<FrozenMappings, Error> {
        // Approximate size of the build data class mappings
        let mut buffer = String::with_capacity(64 * 1024);
        self.load_class_mapping_data(&mut buffer)?;
        sanitize_class_data(&mut buffer);
        Ok(CompactSrgMappingsFormat::parse_text(&buffer)?)
    }
    pub fn read_member_mappings(&self) -> Result<FrozenMappings, Error> {
        // Approximate size of the build data member mappings
        let mut buffer = String::with_capacity(128 * 1024);
        self.load_member_mapping_data(&mut buffer)?;
        Ok(CompactSrgMappingsFormat::parse_text(&buffer)?)
    }
    fn load_class_mapping_data(&self, buffer: &mut String) -> Result<(), Error> {
        let mut path = PathBuf::from("mappings");
        path.push(&self.info.class_mappings);
        self.load(&path, buffer)?;
        Ok(())
    }
    fn load_member_mapping_data(&self, buffer: &mut String) -> Result<(), Error> {
        let mut path = PathBuf::from("mappings");
        path.push(&self.info.member_mappings);
        self.load(&path, buffer)?;
        Ok(())
    }
}