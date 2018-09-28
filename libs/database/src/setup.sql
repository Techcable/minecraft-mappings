/*
 * NOTE: Sqlite has weak support for foreign keys: https://www.sqlite.org/foreignkeys.html
 * Syntax: FOREIGN KEY(<name>) REFERENCES <other_table>(<other_name>)
 */
PRAGMA foreign_keys = ON;
CREATE TABLE minecraft_versions (
    id INTEGER PRIMARY KEY,
    name VARCHAR(64) NOT NULL
);
CREATE UNIQUE INDEX idx_minecraft_versions_name ON minecraft_versions(name);
CREATE TABLE mcp_versions (
    id INTEGER PRIMARY KEY,
    value INTEGER NOT NULL,
    snapshot BOOLEAN NOT NULL,
    minecraft_version INTEGER NOT NULL,
    loaded BOOLEAN NOT NULL,
    FOREIGN KEY(minecraft_version) REFERENCES minecraft_versions(id)
);
CREATE UNIQUE INDEX idx_mcp_versions ON mcp_versions(value, snapshot);
CREATE TABLE method_signatures (
    id INTEGER PRIMARY KEY,
    obf_signature VARCHAR(256) NOT NULL,
    /* Luckily, MCP only uses one signature/class (same as SRG) */
    srg_signature VARCHAR(256),
    spigot_signature VARCHAR(256),
    minecraft_version INTEGER NOT NULL,
    FOREIGN KEY(minecraft_version) REFERENCES minecraft_versions(id)
);
CREATE UNIQUE INDEX idx_method_signatures_obf ON method_signatures(minecraft_version, obf_signature);
CREATE UNIQUE INDEX idx_method_signatures_srg ON method_signatures(minecraft_version, srg_signature);
CREATE UNIQUE INDEX idx_method_signatures_spigot ON method_signatures(minecraft_version, spigot_signature);
/* The obfuscated class names */
CREATE TABLE obf_classes (
    id INTEGER PRIMARY KEY,
    name VARCHAR(256) NOT NULL,
    minecraft_version INTEGER NOT NULL,
    FOREIGN KEY(minecraft_version) REFERENCES minecraft_versions(id)
);
CREATE UNIQUE INDEX idx_obf_classes ON obf_classes(minecraft_version, name);
CREATE TABLE obf_methods (
    id INTEGER PRIMARY KEY,
    declaring_class INTEGER NOT NULL,
    name VARCHAR(128) NOT NULL,
    signature INTEGER NOT NULL,
    minecraft_version INTEGER NOT NULL,
    FOREIGN KEY(declaring_class) REFERENCES obf_classes(id),
    FOREIGN KEY(signature) REFERENCES method_signatures(id)
    FOREIGN KEY(minecraft_version) REFERENCES minecraft_versions(id)
);
CREATE UNIQUE INDEX idx_obf_methods ON obf_methods(minecraft_version, declaring_class, name, signature);
CREATE TABLE obf_fields (
    id INTEGER PRIMARY KEY,
    declaring_class INTEGER NOT NULL,
    name VARCHAR(128) NOT NULL,
    minecraft_version INTEGER NOT NULL,
    FOREIGN KEY(declaring_class) REFERENCES obf_classes(id),
    FOREIGN KEY(minecraft_version) REFERENCES minecraft_versions(id)
);
CREATE UNIQUE INDEX idx_obf_fields ON obf_fields(minecraft_version, declaring_class, name);
/* spigot mapping tables */
CREATE TABLE spigot_classes (
    id INTEGER PRIMARY KEY,
    name VARCHAR(256) NOT NULL,
    obf_class INTEGER NOT NULL,
    FOREIGN KEY(obf_class) REFERENCES obf_classes(id)
);
CREATE UNIQUE INDEX idx_spigot_classes ON spigot_classes(obf_class, name);
CREATE TABLE spigot_methods (
    id INTEGER PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    obf_method INTEGER NOT NULL,
    FOREIGN KEY(obf_method) REFERENCES obf_methods(id)
);
CREATE UNIQUE INDEX idx_spigot_methods ON spigot_methods(obf_method, name);
CREATE TABLE spigot_fields (
    id INTEGER PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    obf_field INTEGER NOT NULL,
    FOREIGN KEY(obf_field) REFERENCES obf_fields(id)
);
CREATE UNIQUE INDEX idx_spigot_fields ON spigot_fields(obf_field, name);
/* srg mapping tables */
CREATE TABLE srg_classes (
    id INTEGER PRIMARY KEY,
    name VARCHAR(256) NOT NULL,
    obf_class INTEGER NOT NULL,
    FOREIGN KEY(obf_class) REFERENCES obf_classes(id)
);

CREATE UNIQUE INDEX idx_srg_classes ON srg_classes(obf_class, name);
CREATE TABLE srg_methods (
    id INTEGER PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    obf_method INTEGER NOT NULL,
    FOREIGN KEY(obf_method) REFERENCES obf_methods(id)
);
CREATE UNIQUE INDEX idx_srg_methods ON srg_methods(obf_method, name);
CREATE TABLE srg_fields (
    id INTEGER PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    obf_field INTEGER NOT NULL,
    FOREIGN KEY(obf_field) REFERENCES obf_fields(id)
);
CREATE UNIQUE INDEX idx_srg_fields ON srg_fields(obf_field, name);
/* mcp mapping tables */
CREATE TABLE mcp_methods (
    id INTEGER PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    obf_method INTEGER NOT NULL,
    mcp_version INTEGER NOT NULL,
    FOREIGN KEY(obf_method) REFERENCES obf_methods(id),
    FOREIGN KEY(mcp_version) REFEREnCES mcp_versions(id)
);
CREATE TABLE mcp_fields (
    id INTEGER PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    obf_field INTEGER NOT NULL,
    mcp_version INTEGER NOT NULL,
    FOREIGN KEY(obf_field) REFERENCES obf_fields(id),
    FOREIGN KEY(mcp_version) REFEREnCES mcp_versions(id)
);
