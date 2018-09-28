#![feature(type_ascription)]
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
use log::{info, debug};

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
    pub fn write_initial_data(&mut self, version: MinecraftVersion) -> Result<(), Error> {
        let cache = self.cache.try_borrow_with(|| {
            MinecraftMappingsCache::setup(self.location.cache_location.clone())
        })?;
        if let Some(writer) = InitialDataWriter::setup(
            self.connection.transaction()?, cache, version)? {
            writer.write_data()?;
        }
        Ok(())
    }
}
pub struct InitialDataWriter<'db> {
    version: MinecraftVersion,
    version_id: i64,
    transaction: Transaction<'db>,
    cache: &'db MinecraftMappingsCache,
    class_ids: IndexMap<ReferenceType, i64>,
    field_ids: IndexMap<FieldData, i64>,
    method_ids: IndexMap<MethodData, i64>,
}
impl<'db> InitialDataWriter<'db> {
    pub fn setup(transaction: Transaction<'db>, cache: &'db MinecraftMappingsCache, version: MinecraftVersion) -> Result<Option<Self>, Error> {
        debug!("Loading data for {}", version);
        let version_name = version.name();
        let version_id: i64;
        {
            let mut version_statement = transaction.prepare(
                "SELECT id FROM minecraft_versions WHERE name = ?")?;
            if version_statement.exists(&[&version_name])? {
                /*
                 * This minecraft version already exists, so it should have its data
                 * When the transaction is dropped,
                 * everything will be rolled back and it'll be like nothing ever happened
                 */
                info!("Already loaded data for {}", version);
                return Ok(None)
            }
            // NOTE: sqlite determines id automatically
            transaction.execute(
                "INSERT INTO minecraft_versions (name) VALUES (?)",
                &[&version.name()]
            )?;
            version_id = version_statement.query_row(
                &[&version.name()],
                |row| row.get(0)
            )?;
        }
        Ok(Some(InitialDataWriter {
            version_id,
            version, transaction,
            cache, class_ids: IndexMap::default(),
            field_ids: IndexMap::default(),
            method_ids: IndexMap::default(),
        }))
    }
    pub fn write_data(mut self) -> Result<(), Error> {
        {
            let version = self.version;
            let version_id = self.version_id;
            debug!("Loading obf data for {}", version);
            // Now load the data and start inserting it into the table
            let data = ObfData::collect(version, self.cache)?;
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
                self.class_ids.insert(obfuscated_class.clone(), class_id);
            }
            drop(select_class_id_statement);
            drop(insert_class_statement);
            let mut insert_field_statement = self.transaction.prepare(
                "INSERT INTO obf_fields (declaring_class, name, minecraft_version) VALUES (?, ?, ?)"
            )?;
            let mut select_field_id_statement = self.transaction.prepare(
                "SELECT id FROM obf_fields WHERE declaring_class = ? AND name = ? AND minecraft_version = ?"
            )?;
            for field in data.fields.iter() {
                let class_id = self.class_ids[field.declaring_type()];
                insert_field_statement.execute(&[&class_id, &field.name(), &version_id])?;
                let field_id: i64 = select_field_id_statement.query_row(
                    &[&class_id, &field.name(), &version_id],
                    |row| row.get(0)
                )?;
                self.field_ids.insert(field.clone(), field_id);
            }
            drop(insert_field_statement);
            let mut signatures = SignatureCache::setup(
                version_id, &self.transaction,
            )?;
            let mut insert_method_statement = self.transaction.prepare(
                "INSERT INTO obf_methods (declaring_class, name, signature, minecraft_version)\
                 VALUES (?, ?, ?, ?)"
            )?;
            let mut select_method_id_statement = self.transaction.prepare(
                "SELECT id FROM obf_methods WHERE declaring_class = ? AND name = ?\
                AND signature = ? AND minecraft_version = ?"
            )?;
            for method in data.methods.iter() {
                let class_id = self.class_ids[method.declaring_type()];
                let signature_id = signatures.load_signature(method.signature())?;
                insert_method_statement.execute(&[
                    &class_id, &method.name,
                    &signature_id, &version_id
                ])?;
                let method_id: i64 = select_method_id_statement.query_row(
                    &[&class_id, &method.name, &signature_id, &version_id],
                    |row| row.get(0)
                )?;
                self.method_ids.insert(method.clone(), method_id);
            }
            info!("Loaded obf data for {}", version);
        }
        {
            /*
             * Now it's time to insert the derived data, which is remapped from the original.
             * Loading these has an implicit assumption that we've already inserted
             * all of the obfuscated classes, fields, methods, and method signatures
             * that they'll ever need to use.
             * Furthermore, they assumed we've loaded it into the corresponding hashmap for .
             * This is a reasonable assumption to make since
             * it's guarenteed by `ObfData::collect`.
             * However, if it's ever violated (due to a bug or corrupted data),
             * the function will panic and the transaction will safely rollback.
             */
            self.write_simple_data(SimpleDataKind::Spigot)?;
            self.write_simple_data(SimpleDataKind::Srg)?;
        }
        // We're finished
        self.transaction.commit()?;
        info!("Successfully loaded OBF data for {}", self.version);
        Ok(())
    }
    fn write_simple_data(&mut self, kind: SimpleDataKind) -> Result<(), Error> {
        let version = self.version;
        debug!("Loading {} data for {}", kind.name(), version);
        {
            let version_id = self.version_id;
            // Now load the mappings and insert it into the table
            let mappings = kind.load_mappings(self.cache, version)?;
            let mut insert_class_statement = self.transaction.prepare(&format!(
                "INSERT INTO {} (name, obf_class) VALUES (?, ?)",
                kind.class_table()
            ))?;
            for (obf_class, remapped_class) in mappings.classes() {
                let obf_class_id = self.class_ids[obf_class];
                insert_class_statement.execute(&[
                    &remapped_class.internal_name(), &obf_class_id
                ])?;
            }
            drop(insert_class_statement);
            let mut insert_field_statement = self.transaction.prepare(&format!(
                "INSERT INTO {} (name, obf_field) VALUES (?, ?)",
                kind.field_table()
            ))?;
            for (obf_field, remapped_field) in mappings.fields() {
                let obf_field_id = self.field_ids[obf_field];
                insert_field_statement.execute(&[&remapped_field.name, &obf_field_id])?;
            }
            drop(insert_field_statement);
            let mut insert_method_statement = self.transaction.prepare(&format!(
                "INSERT INTO {} (name, obf_method) VALUES (?, ?)",
                kind.method_table()
            ))?;
            for (obf_method, remapped_method) in mappings.methods() {
                let obf_method_id = self.method_ids[obf_method];
                insert_method_statement.execute(&[&remapped_method.name, &obf_method_id])?;
            }
            drop(insert_method_statement);
            // Now we have to remap all the signatures using our new mapping data
            let mut load_all_signatures = self.transaction.prepare(
                "SELECT id, obf_signature FROM method_signatures WHERE minecraft_version = ?"
            )?;
            let mut update_signatures = self.transaction.prepare(&format!(
                "UPDATE method_signatures SET {} = ? WHERE id = ? AND minecraft_version = ?",
                kind.signature_column()
            ))?;
            let signatures: Vec<(i64, String)> = load_all_signatures.query_map(&[&version_id], |row| {
                (row.get(0), row.get(1)): (i64, String)
            })?.collect::<Result<_, _>>()?;
            for (id, obf_descriptor) in signatures {
                let obf_signature = MethodSignature::from_descriptor(&obf_descriptor);
                let remapped_signature = obf_signature.transform_class(&mappings);
                update_signatures.execute(&[
                    &remapped_signature.descriptor(),
                    &id,
                    &version_id
                ])?;
            }
        }
        info!("Successfully loaded {} data for {}", kind.name(), version);
        Ok(())
    }
}
enum SimpleDataKind {
    Srg,
    Spigot
}
impl SimpleDataKind {
    fn load_mappings(&self, cache: &MinecraftMappingsCache, version: MinecraftVersion) -> Result<FrozenMappings, Error> {
        match *self {
            SimpleDataKind::Srg => cache.load_srg_mappings(version),
            SimpleDataKind::Spigot => {
                Ok(cache.load_spigot_mappings(version)?
                    .chained_mappings.clone())
            },
        }
    }
    fn name(&self) -> &'static str {
        match *self {
            SimpleDataKind::Srg => "srg",
            SimpleDataKind::Spigot => "spigot",
        }
    }
    fn class_table(&self) -> &'static str {
        match *self {
            SimpleDataKind::Srg => "srg_classes",
            SimpleDataKind::Spigot => "spigot_classes",
        }
    }
    fn field_table(&self) -> &'static str {
        match *self {
            SimpleDataKind::Srg => "srg_fields",
            SimpleDataKind::Spigot => "spigot_fields",
        }
    }
    fn method_table(&self) -> &'static str {
        match *self {
            SimpleDataKind::Srg => "srg_methods",
            SimpleDataKind::Spigot => "spigot_methods",
        }
    }
    fn signature_column(&self) -> &'static str {
        match *self {
            SimpleDataKind::Srg => "srg_signature",
            SimpleDataKind::Spigot => "spigot_signature",
        }
    }
}
struct SignatureCache<'conn> {
    version_id: i64,
    insert_signature_statement: Statement<'conn>,
    select_signature_id_statement: Statement<'conn>,
    cache: IndexMap<MethodSignature, i64>
}
impl<'conn> SignatureCache<'conn> {
    fn setup(version_id: i64, conn: &'conn Connection) -> Result<Self, Error> {
        let insert_signature_statement = conn.prepare(
            "INSERT INTO method_signatures (obf_signature, minecraft_version) VALUES (?, ?)"
        )?;
        let select_signature_id_statement = conn.prepare(
            "SELECT id FROM method_signatures WHERE obf_signature = ? AND minecraft_version = ?"
        )?;
        Ok(SignatureCache {
            version_id, insert_signature_statement,
            select_signature_id_statement,
            cache: IndexMap::default(),
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
        // Must collect all obfuscated data ever used by eight spigot or mcp
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