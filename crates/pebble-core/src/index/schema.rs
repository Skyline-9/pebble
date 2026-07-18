//! `SQLite` schema and sealed-generation validation.

use rusqlite::{Connection, OptionalExtension, params};

use super::IndexError;

pub(super) const SCHEMA_VERSION: u32 = 1;

pub(super) fn create(connection: &Connection, generation: &str) -> Result<(), IndexError> {
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "journal_mode", "delete")?;
    connection.execute_batch(SCHEMA)?;
    connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    connection.execute(
        "INSERT INTO generations(generation_id, schema_version) VALUES(?1, ?2)",
        params![generation, SCHEMA_VERSION],
    )?;
    Ok(())
}

pub(super) fn validate(connection: &Connection, generation: &str) -> Result<(), IndexError> {
    let version = connection.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?;
    if version != SCHEMA_VERSION {
        return Err(IndexError::rebuild("unsupported graph schema version"));
    }
    if super::schema_shape::rows(connection)? != super::schema_shape::expected()? {
        return Err(IndexError::rebuild("SQLite graph schema shape is invalid"));
    }
    let journal_mode =
        connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?;
    if journal_mode.eq_ignore_ascii_case("wal") {
        return Err(IndexError::rebuild(
            "sealed graph cannot use WAL journal mode",
        ));
    }
    let integrity =
        connection.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?;
    if integrity != "ok" {
        return Err(IndexError::rebuild("SQLite integrity check failed"));
    }
    let foreign_key_violation = connection
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
        .optional()?;
    if foreign_key_violation.is_some() {
        return Err(IndexError::rebuild("SQLite foreign key check failed"));
    }
    let mut generations = connection.prepare(
        "SELECT generation_id, schema_version FROM generations ORDER BY generation_id LIMIT 2",
    )?;
    let stored = generations
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if stored.as_slice() != [(generation.to_owned(), SCHEMA_VERSION)] {
        return Err(IndexError::rebuild("generation ownership is invalid"));
    }
    for table in QUERY_TABLES {
        validate_table_ownership(connection, table, generation)?;
    }
    validate_entity_set(connection, generation)?;
    Ok(())
}

fn validate_entity_set(connection: &Connection, generation: &str) -> Result<(), IndexError> {
    let mismatch = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM entities AS entity
            WHERE entity.generation_id = ?1 AND NOT (
                (entity.entity_kind = 'file' AND EXISTS(
                    SELECT 1 FROM files
                    WHERE generation_id = entity.generation_id
                      AND file_id = entity.entity_id
                )) OR
                (entity.entity_kind = 'symbol' AND EXISTS(
                    SELECT 1 FROM symbols
                    WHERE generation_id = entity.generation_id
                      AND symbol_id = entity.entity_id
                )) OR
                (entity.entity_kind = 'chunk' AND EXISTS(
                    SELECT 1 FROM chunks
                    WHERE generation_id = entity.generation_id
                      AND chunk_id = entity.entity_id
                ))
            )
            UNION ALL
            SELECT 1 FROM files
            WHERE generation_id = ?1 AND NOT EXISTS(
                SELECT 1 FROM entities
                WHERE generation_id = files.generation_id
                  AND entity_id = files.file_id AND entity_kind = 'file'
            )
            UNION ALL
            SELECT 1 FROM symbols
            WHERE generation_id = ?1 AND NOT EXISTS(
                SELECT 1 FROM entities
                WHERE generation_id = symbols.generation_id
                  AND entity_id = symbols.symbol_id AND entity_kind = 'symbol'
            )
            UNION ALL
            SELECT 1 FROM chunks
            WHERE generation_id = ?1 AND NOT EXISTS(
                SELECT 1 FROM entities
                WHERE generation_id = chunks.generation_id
                  AND entity_id = chunks.chunk_id AND entity_kind = 'chunk'
            )
        )",
        [generation],
        |row| row.get::<_, bool>(0),
    )?;
    if mismatch {
        return Err(IndexError::rebuild("entity rows are inconsistent"));
    }
    Ok(())
}

fn validate_table_ownership(
    connection: &Connection,
    table: &str,
    generation: &str,
) -> Result<(), IndexError> {
    let sql = format!("SELECT COUNT(*), COUNT(*) FILTER (WHERE generation_id = ?1) FROM {table}");
    let (total, owned) = connection.query_row(&sql, [generation], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    if total != owned {
        return Err(IndexError::rebuild(
            "query-visible graph rows have invalid ownership",
        ));
    }
    Ok(())
}

const QUERY_TABLES: [&str; 9] = [
    "repositories",
    "revisions",
    "files",
    "entities",
    "symbols",
    "chunks",
    "edges",
    "diagnostics",
    "metadata",
];

pub(super) const SCHEMA: &str = "
CREATE TABLE generations (
    generation_id TEXT PRIMARY KEY NOT NULL,
    schema_version INTEGER NOT NULL CHECK(schema_version = 1)
);
CREATE TABLE repositories (
    generation_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    PRIMARY KEY(generation_id, repository_id),
    FOREIGN KEY(generation_id) REFERENCES generations(generation_id)
);
CREATE TABLE revisions (
    generation_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    revision TEXT NOT NULL,
    base_oid TEXT NOT NULL,
    dirty_digest TEXT,
    PRIMARY KEY(generation_id, repository_id, revision),
    FOREIGN KEY(generation_id, repository_id)
        REFERENCES repositories(generation_id, repository_id)
);
CREATE TABLE files (
    generation_id TEXT NOT NULL,
    file_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    revision TEXT NOT NULL,
    path TEXT NOT NULL,
    language TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    PRIMARY KEY(generation_id, file_id),
    UNIQUE(generation_id, repository_id, revision, path),
    FOREIGN KEY(generation_id, repository_id, revision)
        REFERENCES revisions(generation_id, repository_id, revision)
);
CREATE INDEX files_by_path ON files(generation_id, path);
CREATE TABLE entities (
    generation_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    entity_kind TEXT NOT NULL CHECK(entity_kind IN ('file', 'symbol', 'chunk')),
    PRIMARY KEY(generation_id, entity_id),
    FOREIGN KEY(generation_id) REFERENCES generations(generation_id)
);
CREATE TABLE symbols (
    generation_id TEXT NOT NULL,
    symbol_id TEXT NOT NULL,
    file_id TEXT NOT NULL,
    name TEXT NOT NULL,
    symbol_kind TEXT NOT NULL,
    start_line INTEGER NOT NULL CHECK(start_line > 0),
    end_line INTEGER NOT NULL CHECK(end_line >= start_line),
    PRIMARY KEY(generation_id, symbol_id),
    FOREIGN KEY(generation_id, symbol_id)
        REFERENCES entities(generation_id, entity_id),
    FOREIGN KEY(generation_id, file_id)
        REFERENCES files(generation_id, file_id)
);
CREATE INDEX symbols_by_name ON symbols(generation_id, name);
CREATE TABLE chunks (
    generation_id TEXT NOT NULL,
    chunk_id TEXT NOT NULL,
    file_id TEXT NOT NULL,
    start_line INTEGER NOT NULL CHECK(start_line > 0),
    end_line INTEGER NOT NULL CHECK(end_line >= start_line),
    content TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    PRIMARY KEY(generation_id, chunk_id),
    FOREIGN KEY(generation_id, chunk_id)
        REFERENCES entities(generation_id, entity_id),
    FOREIGN KEY(generation_id, file_id)
        REFERENCES files(generation_id, file_id)
);
CREATE TABLE edges (
    generation_id TEXT NOT NULL,
    edge_kind TEXT NOT NULL CHECK(edge_kind IN
        ('defines', 'references', 'imports', 'calls', 'contains')),
    source_id TEXT NOT NULL,
    target_entity_id TEXT,
    external_target TEXT,
    source_line INTEGER NOT NULL CHECK(source_line > 0),
    CHECK((target_entity_id IS NULL) != (external_target IS NULL)),
    FOREIGN KEY(generation_id, source_id)
        REFERENCES entities(generation_id, entity_id),
    FOREIGN KEY(generation_id, target_entity_id)
        REFERENCES entities(generation_id, entity_id)
);
CREATE INDEX edges_by_source ON edges(generation_id, source_id);
CREATE INDEX edges_by_target ON edges(generation_id, target_entity_id, external_target);
CREATE INDEX edges_by_kind ON edges(generation_id, edge_kind);
CREATE UNIQUE INDEX edges_unique ON edges(
    generation_id, edge_kind, source_id,
    COALESCE(target_entity_id, ''), COALESCE(external_target, ''), source_line
);
CREATE TABLE diagnostics (
    generation_id TEXT NOT NULL,
    path TEXT,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    FOREIGN KEY(generation_id) REFERENCES generations(generation_id)
);
CREATE UNIQUE INDEX diagnostics_unique ON diagnostics(
    generation_id, COALESCE(path, ''), code, message
);
CREATE TABLE metadata (
    generation_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY(generation_id, key),
    FOREIGN KEY(generation_id) REFERENCES generations(generation_id)
);
";
