//! `SQLite` evidence graph integration tests.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::domain::{
    ChunkId, FileId, GenerationId, RepositoryId, SymbolId, WorktreeRevision,
};
use pebble_core::index::{
    EdgeTarget, GenerationBuilder, GenerationReader, GraphEdgeKind, GraphReadLimit, IndexError,
};

#[test]
fn creates_normalized_schema_v1_with_indices_and_foreign_keys()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("schema")?;
    let builder = GenerationBuilder::create(directory.path(), generation("schema")?)?;
    let connection = rusqlite::Connection::open(builder.graph_path())?;

    assert_eq!(
        connection.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?,
        1
    );
    assert_ne!(
        connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?,
        "wal"
    );
    for table in [
        "generations",
        "repositories",
        "revisions",
        "files",
        "symbols",
        "chunks",
        "entities",
        "edges",
        "diagnostics",
        "metadata",
    ] {
        let present: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        )?;
        assert!(present, "missing table {table}");
    }
    for index in [
        "files_by_path",
        "symbols_by_name",
        "edges_by_source",
        "edges_by_target",
        "edges_by_kind",
    ] {
        let present: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
            [index],
            |row| row.get(0),
        )?;
        assert!(present, "missing index {index}");
    }
    Ok(())
}

#[test]
fn enforces_foreign_keys_and_generation_ownership() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("foreign-keys")?;
    let builder = GenerationBuilder::create(directory.path(), generation("one")?)?;
    let repository = repository()?;
    let revision = revision()?;
    let file = FileId::derive(&repository, "src/lib.rs");

    assert!(
        builder
            .graph()
            .insert_file(&file, &repository, &revision, "src/lib.rs", "rust", "abcd")
            .is_err()
    );

    let connection = rusqlite::Connection::open(builder.graph_path())?;
    connection.pragma_update(None, "foreign_keys", true)?;
    let error = connection.execute(
        "INSERT INTO repositories(generation_id, repository_id, display_name)
         VALUES('another-generation', 'repo', 'Repo')",
        [],
    );
    assert!(error.is_err());
    Ok(())
}

#[test]
fn inserts_all_rows_idempotently_and_reads_all_edge_kinds() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TestDirectory::new("rows")?;
    let builder = GenerationBuilder::create(directory.path(), generation("rows")?)?;
    insert_fixture(builder.graph())?;
    insert_fixture(builder.graph())?;
    let reader = builder.seal()?.activate()?;

    let counts = reader.graph().counts()?;
    assert_eq!(counts.repositories(), 1);
    assert_eq!(counts.revisions(), 1);
    assert_eq!(counts.files(), 1);
    assert_eq!(counts.symbols(), 1);
    assert_eq!(counts.chunks(), 1);
    assert_eq!(counts.edges(), 5);
    assert_eq!(counts.diagnostics(), 1);
    assert_eq!(
        reader.graph().metadata("extractor")?.as_deref(),
        Some("tree-sitter")
    );

    for kind in GraphEdgeKind::ALL {
        let edges = reader.graph().edges(kind, GraphReadLimit::try_from(10)?)?;
        assert_eq!(edges.len(), 1, "missing {kind:?}");
        assert_eq!(edges[0].kind(), kind);
    }
    Ok(())
}

#[test]
fn graph_reads_require_a_validated_sql_limit() -> Result<(), Box<dyn std::error::Error>> {
    assert!(GraphReadLimit::try_from(0).is_err());
    assert!(GraphReadLimit::try_from(10_001).is_err());

    let directory = TestDirectory::new("bounded-reads")?;
    let builder = GenerationBuilder::create(directory.path(), generation("bounded")?)?;
    insert_fixture(builder.graph())?;
    builder.graph().insert_edge(
        GraphEdgeKind::Imports,
        FileId::derive(&repository()?, "src/lib.rs").as_str(),
        EdgeTarget::External("std::io"),
        2,
    )?;
    let reader = builder.seal()?.activate()?;

    let edges = reader
        .graph()
        .edges(GraphEdgeKind::Imports, GraphReadLimit::try_from(1)?)?;
    assert_eq!(edges.len(), 1);
    Ok(())
}

#[test]
fn conflicting_same_identity_payloads_fail_without_changing_original_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("idempotency-conflicts")?;
    let builder = GenerationBuilder::create(directory.path(), generation("conflicts")?)?;
    insert_fixture(builder.graph())?;

    let repository = repository()?;
    let revision = revision()?;
    let file = FileId::derive(&repository, "src/lib.rs");
    let symbol = SymbolId::derive(&repository, "rust", "src/lib.rs:answer");
    let chunk = ChunkId::derive(&file, 1, 3, 0, "beef");

    assert!(
        builder
            .graph()
            .insert_repository(&repository, "Changed")
            .is_err()
    );
    assert!(
        builder
            .graph()
            .insert_file(
                &file,
                &repository,
                &revision,
                "src/lib.rs",
                "python",
                "changed"
            )
            .is_err()
    );
    assert!(
        builder
            .graph()
            .insert_symbol(&symbol, &file, "changed", "class", 1, 4)
            .is_err()
    );
    assert!(
        builder
            .graph()
            .insert_chunk(&chunk, &file, 1, 4, "changed", "changed")
            .is_err()
    );
    assert!(
        builder
            .graph()
            .set_metadata("extractor", "different")
            .is_err()
    );

    let reader = builder.seal()?.activate()?;
    assert_eq!(reader.graph().counts()?.repositories(), 1);
    assert_eq!(
        reader.graph().metadata("extractor")?.as_deref(),
        Some("tree-sitter")
    );
    Ok(())
}

#[test]
fn divergent_entity_kind_with_the_same_identity_fails() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("entity-kind-conflict")?;
    let builder = GenerationBuilder::create(directory.path(), generation("entity-kind")?)?;
    let repository = repository()?;
    let revision = revision()?;
    let file = FileId::derive(&repository, "src/lib.rs");
    let conflicting_symbol = SymbolId::try_from(file.as_str().to_owned())?;
    builder.graph().insert_repository(&repository, "Pebble")?;
    builder.graph().insert_revision(&repository, &revision)?;
    builder
        .graph()
        .insert_file(&file, &repository, &revision, "src/lib.rs", "rust", "abcd")?;

    assert!(
        builder
            .graph()
            .insert_symbol(&conflicting_symbol, &file, "answer", "function", 1, 1)
            .is_err()
    );
    Ok(())
}

#[test]
fn sealing_rejects_extra_owners_and_schema_shape_changes() -> Result<(), Box<dyn std::error::Error>>
{
    let owners = TestDirectory::new("multiple-owners")?;
    let owner_builder = GenerationBuilder::create(owners.path(), generation("owner")?)?;
    let owner_connection = rusqlite::Connection::open(owner_builder.graph_path())?;
    owner_connection.execute(
        "INSERT INTO generations(generation_id, schema_version) VALUES('other', 1)",
        [],
    )?;
    owner_connection.execute(
        "INSERT INTO repositories VALUES('other', 'other-repository', 'Other')",
        [],
    )?;
    drop(owner_connection);
    assert!(matches!(
        owner_builder.seal(),
        Err(IndexError::RebuildRequired(_))
    ));

    let schema = TestDirectory::new("schema-shape")?;
    let schema_builder = GenerationBuilder::create(schema.path(), generation("shape")?)?;
    let schema_connection = rusqlite::Connection::open(schema_builder.graph_path())?;
    schema_connection.execute("DROP INDEX edges_by_kind", [])?;
    schema_connection.execute(
        "CREATE INDEX edges_by_kind ON edges(generation_id, source_line)",
        [],
    )?;
    drop(schema_connection);
    assert!(matches!(
        schema_builder.seal(),
        Err(IndexError::RebuildRequired(_))
    ));
    Ok(())
}

#[test]
fn sealing_rejects_phantom_missing_and_wrong_kind_entities()
-> Result<(), Box<dyn std::error::Error>> {
    for (label, mutation) in [
        (
            "phantom",
            "INSERT INTO entities VALUES('entities', 'phantom', 'file')",
        ),
        ("missing", "DELETE FROM entities WHERE entity_kind = 'file'"),
        (
            "wrong-kind",
            "UPDATE entities SET entity_kind = 'symbol' WHERE entity_kind = 'file'",
        ),
    ] {
        let directory = TestDirectory::new(label)?;
        let builder = GenerationBuilder::create(directory.path(), generation("entities")?)?;
        insert_fixture(builder.graph())?;
        let connection = rusqlite::Connection::open(builder.graph_path())?;
        connection.pragma_update(None, "foreign_keys", false)?;
        connection.execute(mutation, [])?;
        drop(connection);

        assert!(matches!(
            builder.seal(),
            Err(IndexError::RebuildRequired(_))
        ));
    }
    Ok(())
}

#[test]
fn sealing_rejects_unbounded_schema_objects() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("schema-object-limit")?;
    let builder = GenerationBuilder::create(directory.path(), generation("schema-limit")?)?;
    let connection = rusqlite::Connection::open(builder.graph_path())?;
    for sequence in 0..256 {
        connection.execute(
            &format!("CREATE TABLE unexpected_{sequence}(value TEXT)"),
            [],
        )?;
    }
    drop(connection);

    assert!(matches!(
        builder.seal(),
        Err(IndexError::RebuildRequired(_))
    ));
    Ok(())
}

#[test]
fn sealing_rejects_wal_mode_and_sidecars() -> Result<(), Box<dyn std::error::Error>> {
    let wal = TestDirectory::new("persisted-wal")?;
    let wal_builder = GenerationBuilder::create(wal.path(), generation("wal")?)?;
    let connection = rusqlite::Connection::open(wal_builder.graph_path())?;
    assert_eq!(
        connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))?,
        "wal"
    );
    drop(connection);
    assert!(matches!(
        wal_builder.seal(),
        Err(IndexError::RebuildRequired(_))
    ));

    for suffix in ["-wal", "-shm"] {
        let sidecar = TestDirectory::new("sqlite-sidecar")?;
        let builder = GenerationBuilder::create(sidecar.path(), generation("sidecar")?)?;
        fs::write(
            format!("{}{suffix}", builder.graph_path().display()),
            b"sidecar",
        )?;
        assert!(matches!(
            builder.seal(),
            Err(IndexError::RebuildRequired(_))
        ));
    }
    Ok(())
}

#[test]
fn rolls_back_the_whole_explicit_transaction_on_failure() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TestDirectory::new("rollback")?;
    let builder = GenerationBuilder::create(directory.path(), generation("rollback")?)?;
    let repository = repository()?;
    let revision = revision()?;
    let file = FileId::derive(&repository, "src/lib.rs");

    let result = builder.graph().transaction(|transaction| {
        transaction.insert_repository(&repository, "Pebble")?;
        transaction.insert_file(&file, &repository, &revision, "src/lib.rs", "rust", "abcd")
    });
    assert!(matches!(result, Err(IndexError::Sqlite(_))));

    let reader = builder.seal()?.activate()?;
    assert_eq!(reader.graph().counts()?.repositories(), 0);
    Ok(())
}

#[test]
fn rejects_invalid_rows_instead_of_silently_ignoring() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("constraints")?;
    let builder = GenerationBuilder::create(directory.path(), generation("constraints")?)?;
    let repository = repository()?;
    let revision = revision()?;
    let file = FileId::derive(&repository, "src/lib.rs");
    let symbol = SymbolId::derive(&repository, "rust", "bad-range");
    builder.graph().insert_repository(&repository, "Pebble")?;
    builder.graph().insert_revision(&repository, &revision)?;
    builder
        .graph()
        .insert_file(&file, &repository, &revision, "src/lib.rs", "rust", "abcd")?;

    assert!(
        builder
            .graph()
            .insert_symbol(&symbol, &file, "bad", "function", 0, 0)
            .is_err()
    );
    Ok(())
}

#[test]
fn opens_generation_queries_read_only() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("read-only")?;
    let builder = GenerationBuilder::create(directory.path(), generation("readonly")?)?;
    insert_fixture(builder.graph())?;
    let reader = builder.seal()?.activate()?;

    assert!(reader.graph().is_read_only()?);
    let write = rusqlite::Connection::open_with_flags(
        reader.graph_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?
    .execute("DELETE FROM files", []);
    assert!(write.is_err());

    let reopened = GenerationReader::open(directory.path(), reader.id().clone())?;
    assert!(reopened.graph().is_read_only()?);
    assert_eq!(reopened.graph().counts()?.files(), 1);
    Ok(())
}

fn insert_fixture(graph: &pebble_core::index::GraphWriter) -> Result<(), IndexError> {
    let repository = repository().map_err(IndexError::Domain)?;
    let revision = revision().map_err(IndexError::Domain)?;
    let file = FileId::derive(&repository, "src/lib.rs");
    let symbol = SymbolId::derive(&repository, "rust", "src/lib.rs:answer");
    let chunk = ChunkId::derive(&file, 1, 3, 0, "beef");

    graph.insert_repository(&repository, "Pebble")?;
    graph.insert_revision(&repository, &revision)?;
    graph.insert_file(&file, &repository, &revision, "src/lib.rs", "rust", "abcd")?;
    graph.insert_symbol(&symbol, &file, "answer", "function", 1, 3)?;
    graph.insert_chunk(&chunk, &file, 1, 3, "fn answer() {}", "beef")?;
    graph.insert_edge(
        GraphEdgeKind::Defines,
        file.as_str(),
        EdgeTarget::Entity(symbol.as_str()),
        1,
    )?;
    graph.insert_edge(
        GraphEdgeKind::References,
        file.as_str(),
        EdgeTarget::Entity(symbol.as_str()),
        2,
    )?;
    graph.insert_edge(
        GraphEdgeKind::Imports,
        file.as_str(),
        EdgeTarget::External("std::fmt"),
        1,
    )?;
    graph.insert_edge(
        GraphEdgeKind::Calls,
        file.as_str(),
        EdgeTarget::External("println"),
        3,
    )?;
    graph.insert_edge(
        GraphEdgeKind::Contains,
        file.as_str(),
        EdgeTarget::Entity(chunk.as_str()),
        1,
    )?;
    graph.insert_diagnostic(Some("src/lib.rs"), "parse_recovery", "recovered")?;
    graph.set_metadata("extractor", "tree-sitter")?;
    Ok(())
}

fn repository() -> Result<RepositoryId, pebble_core::error::DomainError> {
    RepositoryId::try_from("acme.pebble".to_owned())
}

fn revision() -> Result<WorktreeRevision, pebble_core::error::DomainError> {
    WorktreeRevision::clean("0123456789abcdef")
}

fn generation(value: &str) -> Result<GenerationId, pebble_core::error::DomainError> {
    GenerationId::try_from(value.to_owned())
}

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-graph-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        let path = path.canonicalize()?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
