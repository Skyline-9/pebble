//! Deterministic lexical directory replacement tests.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::domain::{ChunkId, FileId, RepositoryId, WorktreeRevision};

use super::generation_races::{RacePoint, inject};
use super::{LexicalReader, LexicalWriter};

#[cfg(unix)]
#[test]
fn writer_commit_stays_on_validated_directory_after_aba_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new("writer-commit")?;
    let lexical = root.path().join("lexical");
    fs::create_dir(&lexical)?;
    let mut writer = LexicalWriter::create(&lexical)?;
    let expected = add_chunk(&mut writer, "original", "original_term")?;
    let original = root.path().join("original");
    let replacement = lexical.clone();
    inject(RacePoint::LexicalCommit, move |_, path| {
        assert!(fs::rename(path, &original).is_ok());
        assert!(fs::create_dir(&replacement).is_ok());
    });

    let reader = writer.finish()?;

    assert_eq!(
        reader.exact_identifier("original_term", 10)?[0].entity_id(),
        expected
    );
    assert!(fs::read_dir(&lexical)?.next().is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn reader_open_stays_on_validated_directory_after_aba_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new("reader-open")?;
    let lexical = root.path().join("lexical");
    fs::create_dir(&lexical)?;
    let mut original_writer = LexicalWriter::create(&lexical)?;
    let expected = add_chunk(&mut original_writer, "original", "original_term")?;
    drop(original_writer.finish()?);

    let replacement_root = TestRoot::new("reader-open-replacement")?;
    let replacement = replacement_root.path().join("lexical");
    fs::create_dir(&replacement)?;
    let mut replacement_writer = LexicalWriter::create(&replacement)?;
    add_chunk(&mut replacement_writer, "replacement", "replacement_term")?;
    drop(replacement_writer.finish()?);

    let displaced = root.path().join("original");
    inject(RacePoint::LexicalReaderOpen, move |_, path| {
        assert!(fs::rename(path, &displaced).is_ok());
        assert!(fs::rename(&replacement, path).is_ok());
    });

    let reader = LexicalReader::open(&lexical)?;

    assert_eq!(
        reader.exact_identifier("original_term", 10)?[0].entity_id(),
        expected
    );
    assert!(reader.exact_identifier("replacement_term", 10)?.is_empty());
    Ok(())
}

fn add_chunk(
    writer: &mut LexicalWriter,
    label: &str,
    body: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let repository = RepositoryId::try_from(format!("acme.{label}"))?;
    let revision = WorktreeRevision::clean("0123456789abcdef")?;
    let file = FileId::derive(&repository, "src/lib.rs");
    let chunk = ChunkId::derive(&file, 1, 1, 0, body);
    writer.add_chunk(
        &chunk,
        &repository,
        &revision,
        "src/lib.rs",
        "rust",
        body,
        1,
        1,
    )?;
    Ok(chunk.as_str().to_owned())
}

struct TestRoot(std::path::PathBuf);

impl TestRoot {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-lexical-race-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
