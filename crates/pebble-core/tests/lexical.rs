#![forbid(unsafe_code)]

//! Tantivy lexical generation integration tests.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::domain::{ChunkId, FileId, RepositoryId, SymbolId, WorktreeRevision};
use pebble_core::index::{IndexError, LexicalWriter};

#[test]
fn indexes_text_exact_metadata_and_code_identifiers() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("fields")?;
    let repository = RepositoryId::try_from("acme.pebble".to_owned())?;
    let revision = WorktreeRevision::clean("0123456789abcdef")?;
    let code_file = FileId::derive(&repository, "src/http_client.rs");
    let note_file = FileId::derive(&repository, "notes/architecture.md");
    let chunk_id = ChunkId::derive(&code_file, 1, 3, 0, "code");
    let note_id = ChunkId::derive(&note_file, 5, 5, 0, "note");
    let symbol_id = SymbolId::derive(&repository, "rust", "HttpClient");
    let mut writer = LexicalWriter::create(directory.path())?;

    writer.add_chunk(
        &chunk_id,
        &repository,
        &revision,
        "src/http_client.rs",
        "rust",
        "fn parse_http_response() { HttpClient::send(); }",
        1,
        3,
    )?;
    writer.add_chunk(
        &note_id,
        &repository,
        &revision,
        "notes/architecture.md",
        "markdown",
        "The durable lexical compiler keeps generations immutable.",
        5,
        5,
    )?;
    writer.add_symbol(
        &symbol_id,
        &repository,
        &revision,
        "src/http_client.rs",
        "rust",
        "HttpClient",
        "struct HttpClient;",
        7,
        7,
    )?;
    let reader = writer.finish()?;

    assert_eq!(reader.search_text("durable compiler", 10)?.len(), 1);
    assert_eq!(reader.exact_path("src/http_client.rs", 10)?.len(), 2);
    assert_eq!(reader.exact_symbol("HttpClient", 10)?.len(), 1);
    assert!(
        reader
            .exact_identifier("parse_http_response", 10)?
            .iter()
            .any(|hit| hit.entity_id() == chunk_id.as_str())
    );
    let hit = &reader.exact_symbol("HttpClient", 10)?[0];
    assert_eq!(hit.repository(), repository.as_str());
    assert_eq!(hit.revision(), revision.to_string());
    assert_eq!(hit.path(), "src/http_client.rs");
    assert_eq!(hit.language(), "rust");
    assert_eq!(hit.symbol(), Some("HttpClient"));
    assert_eq!(hit.start_line(), 7);
    assert_eq!(hit.end_line(), 7);
    assert_eq!(hit.kind(), "symbol");
    Ok(())
}

#[test]
fn stable_documents_survive_reopen_and_have_deterministic_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("stable")?;
    let repository = RepositoryId::try_from("acme.stable".to_owned())?;
    let revision = WorktreeRevision::dirty("abcdef", "123456")?;
    let file = FileId::derive(&repository, "src/lib.rs");
    let chunk = ChunkId::derive(&file, 1, 1, 0, "same");
    let mut writer = LexicalWriter::create(directory.path())?;
    writer.add_chunk(
        &chunk,
        &repository,
        &revision,
        "src/lib.rs",
        "rust",
        "stable_identity",
        1,
        1,
    )?;
    let reader = writer.finish()?;
    let first = reader.exact_identifier("stable_identity", 10)?;
    drop(reader);

    let reopened = pebble_core::index::LexicalReader::open(directory.path())?;
    let second = reopened.exact_identifier("stable_identity", 10)?;

    assert_eq!(first, second);
    assert_eq!(second[0].entity_id(), chunk.as_str());
    assert_eq!(reopened.document_count(), 1);
    Ok(())
}

#[test]
fn rejects_oversized_queries_before_tokenization() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("oversized-query")?;
    let reader = LexicalWriter::create(directory.path())?.finish()?;
    let oversized = "x".repeat(16 * 1024 + 1);

    assert!(matches!(
        reader.search_text(&oversized, 10),
        Err(IndexError::QueryTooLarge {
            maximum: 16_384,
            ..
        })
    ));
    assert!(matches!(
        reader.exact_path(&oversized, 10),
        Err(IndexError::QueryTooLarge {
            maximum: 16_384,
            ..
        })
    ));
    Ok(())
}

#[test]
fn rejects_too_many_unique_query_terms() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("query-terms")?;
    let reader = LexicalWriter::create(directory.path())?.finish()?;
    let query = (0..257)
        .map(|term| format!("term_{term}"))
        .collect::<Vec<_>>()
        .join(" ");

    assert!(matches!(
        reader.search_text(&query, 10),
        Err(IndexError::TooManyQueryTerms { maximum: 256 })
    ));
    Ok(())
}

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-lexical-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
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
