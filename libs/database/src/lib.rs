extern crate rusqlite;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
extern crate lazycell;
extern crate failure;
extern crate srglib;
extern crate indexmap;
extern crate log;


#[cfg(dummy)] // For intellij
extern crate minecraft_mappings_core as mappings;
#[cfg(dummy)] // For intellij
extern crate minecraft_mappings_engine as engine;

use std::iter::Extend;
use std::path::{PathBuf};
use std::fs::{self, File};

use indexmap::{IndexMap, IndexSet};
use failure::{Error, bail};
use rusqlite::{Connection, Transaction, Statement};
use serde_derive::{Serialize, Deserialize};
use failure_derive::Fail;
use lazycell::LazyCell;
use log::{info, trace};

use mappings::MinecraftVersion;
use mappings::cache::MinecraftMappingsCache;
use srglib::prelude::*;

pub struct DatabaseLocation {
    database_location: PathBuf,
    cache_location: PathBuf
}
impl DatabaseLocation {
    #[inline]
    pub fn new(database_location: PathBuf, cache_location: PathBuf) -> Result<DatabaseLocation, Error> {
        fs::create_dir_all(&database_location)?;
        // NOTE: Don't eagerly create cache directory since we might not even need it
        if cache_location.is_file() {
            bail!("Cache directory is actually a file: {}", cache_location.display())
        }
        Ok(DatabaseLocation { database_location, cache_location })
    }
    #[inline]
    fn database_file(&self) -> PathBuf {
        self.database_location.join("mappings.sqlite")
    }
    #[inline]
    fn state_file(&self) -> PathBuf {
        self.database_location.join("state.json")
    }
    fn load_state(&self) -> Result<DatabaseState, Error> {
        if !self.state_file().exists() {
            let mut file = File::create(self.state_file())?;
            let state = DatabaseState::default();
            ::serde_json::to_writer_pretty(&mut file, &state)?;
            file.sync_all()?;
            Ok(state)
        } else {
            let file = File::open(self.state_file())?;
            Ok(::serde_json::from_reader(file)?)
        }
    }
    fn write_state(&self, state: DatabaseState) -> Result<(), Error> {
        let mut file = File::create(self.state_file())?;
        ::serde_json::to_writer_pretty(&mut file, &state)?;
        file.sync_all()?;
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
struct DatabaseState {
    version: u32,
}

#[derive(Debug, Fail)]
#[fail(display = "Invalid database version, expected {} but got {}", expected, actual)]
pub struct UnexpectedDatabaseVersion {
    expected: u32,
    actual: u32
}

const CURRENT_DATABASE_VERSION: u32 = 1;
pub struct MappingsDatabase {
    connection: Connection,
    location: DatabaseLocation,
    cache: LazyCell<MinecraftMappingsCache>
}
impl MappingsDatabase {
    pub fn open(location: DatabaseLocation) -> Result<MappingsDatabase, Error> {
        let connection = Connection::open(location.database_file())?;
        // Execute any 'migrations' we need
        let mut state = location.load_state()?;
        if state.version == 0 {
            info!("Migrating from v0 -> v1");
            connection.execute_batch(include_str!("setup.sql"))?;
            state.version = 1;
        }
        if state.version != CURRENT_DATABASE_VERSION {
            return Err(UnexpectedDatabaseVersion {
                expected: CURRENT_DATABASE_VERSION,
                actual: state.version
            }.into())
        }
        debug!("Connecting to database with version v{}", state.version);
        location.write_state(state)?;
        Ok(MappingsDatabase { connection, location, cache: LazyCell::new() })
    }
    pub fn create_writer(&mut self) -> Result<MappingsDatabaseWriter, Error> {
        let cache = self.cache.try_borrow_with(|| {
            MinecraftMappingsCache::setup(self.location.cache_location.clone())
        })?;
        Ok(MappingsDatabaseWriter { cache, transaction: self.connection.transaction()? })
    }
}
pub struct MappingsDatabaseWriter<'db> {
    transaction: Transaction<'db>,
    cache: &'db MinecraftMappingsCache
}
impl<'db> MappingsDatabaseWriter<'db> {
    pub fn write_obf_data(self, version: MinecraftVersion) -> Result<(), Error> {
        debug!("Loading OBF data for {}", version);
        {
            let version_name = format!("{}", version);
            let mut version_statement = self.transaction.prepare(
                "SELECT id FROM minecraft_versions WHERE name = ?")?;
            if version_statement.exists(&[&version_name])? {
                /*
                 * This minecraft version already exists, so it should have its data
                 * When we're dropped, the transaction will be rolled back and everything will be fine
                 */
                info!("Already loaded OBF data for {}", version);
                return Ok(())
            }
            // NOTE: sqlite determines id automatically
            self.transaction.execute(
                "INSERT INTO minecraft_versions (name) VALUES (?)",
                &[&version_name]
            )?;
            let version_id: i64 = version_statement.query_row(
                &[&version_name],
                |row| row.get(0)
            )?;
            // Now load the data and start inserting it into the table
            let data = ObfData::collect(version, self.cache)?;
            let mut class_ids = IndexMap::new();
            let mut insert_class_statement = self.transaction.prepare(
                "INSERT INTO obf_classes (name, minecraft_version) VALUES (?, ?)"
            )?;
            let mut select_class_id_statement = self.transaction.prepare(
                "SELECT id FROM obf_classes WHERE name = ? AND minecraft_version = ?"
            )?;
            for obfuscated_class in data.classes.iter() {
                let name = obfuscated_class.internal_name();
                insert_class_statement.execute(&[&name, &version_id])?;
                let class_id: i64 = select_class_id_statement.query_row(
                    &[&name, &version_id],
                    |row| row.get(0)
                )?;
                class_ids.insert(obfuscated_class, class_id);
            }
            drop(select_class_id_statement);
            drop(insert_class_statement);
            let mut insert_field_statement = self.transaction.prepare(
                "INSERT INTO obf_fields (declaring_class, name, minecraft_version) VALUES (?, ?, ?)"
            )?;
            for field in data.fields.iter() {
                let class_id = class_ids[field.declaring_type()];
                insert_field_statement.execute(&[&class_id, &field.name(), &version_id])?;
            }
            drop(insert_field_statement);
            let mut signatures = ObfSignatureCache::setup(version_id, &self.transaction)?;
            let mut insert_method_statement = self.transaction.prepare(
                "INSERT INTO obf_methods (declaring_class, name, signature, minecraft_version)\
                 VALUES (?, ?, ?, ?)"
            )?;
            for method in data.methods.iter() {
                let class_id = class_ids[method.declaring_type()];
                let signature_id = signatures.load_signature(method.signature())?;
                insert_method_statement.execute(&[
                    &class_id, &method.name,
                    &signature_id, &version_id
                ])?;
            }
            drop(signatures);
            drop(insert_method_statement);
        }
        // We're finished
        self.transaction.commit()?;
        info!("Successfully loaded OBF data for {}", version);
        Ok(())
    }

}
struct ObfSignatureCache<'conn> {
    version_id: i64,
    insert_signature_statement: Statement<'conn>,
    select_signature_id_statement: Statement<'conn>,
    cache: IndexMap<MethodSignature, i64>
}
impl<'conn> ObfSignatureCache<'conn> {
    fn setup(version_id: i64, conn: &'conn Connection) -> Result<Self, Error> {
        let insert_signature_statement = conn.prepare(
            "INSERT INTO method_signatures (obf_signature, minecraft_version) VALUES (?, ?)"
        )?;
        let select_signature_id_statement = conn.prepare(
            "SELECT id FROM method_signatures WHERE obf_signature = ? AND minecraft_version = ?"
        )?;
        Ok(ObfSignatureCache {
            version_id, insert_signature_statement,
            select_signature_id_statement, cache: IndexMap::default()
        })
    }
    fn load_signature(&mut self, signature: &MethodSignature) -> Result<i64, Error> {
        if let Some(&id) = self.cache.get(signature) {
            return Ok(id)
        }
        let id = self.fallback_load_signature(signature)?;
        self.cache.insert(signature.clone(), id);
        Ok(id)
    }
    fn fallback_load_signature(&mut self, signature: &MethodSignature) -> Result<i64, Error> {
        if !self.select_signature_id_statement.exists(
            &[&signature.descriptor(), &self.version_id])? {
            self.insert_signature_statement.execute(
                &[&signature.descriptor(), &self.version_id])?;
        }
        let id: i64 = self.select_signature_id_statement
            .query_row(&[&signature.descriptor(), &self.version_id], |row| row.get(0))?;
        Ok(id)
    }
}

#[derive(Debug, Default)]
struct ObfData {
    classes: IndexSet<ReferenceType>,
    fields: IndexSet<FieldData>,
    methods: IndexSet<MethodData>
}
impl ObfData {
    fn collect(version: MinecraftVersion, cache: &MinecraftMappingsCache) -> Result<ObfData, Error> {
        let mut data = ObfData::default();
        data.load(&cache.load_srg_mappings(version)?);
        data.load(&cache.load_spigot_mappings(version)?.chained_mappings);
        Ok(data)
    }
    fn load(&mut self, mappings: &FrozenMappings) {
        self.classes.extend(mappings.original_classes().cloned());
        self.fields.extend(mappings.original_fields().cloned());
        self.methods.extend(mappings.original_methods().cloned());
    }

}