#![forbid(unsafe_code)]

//! Integration tests for personal knowledge notes and promotion.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::personal::{PersonalNoteStore, promote, promote_confirmed};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-personal-{label}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn create_list_and_read_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let personal = TempDir::new("round-trip")?;

    let created = PersonalNoteStore::create(personal.path(), "Authentication notes")?;
    assert_eq!(created.title(), "Authentication notes");
    assert!(created.id().starts_with("note_"));

    let listed = PersonalNoteStore::list(personal.path())?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id(), created.id());
    assert_eq!(listed[0].content(), created.content());

    let read = PersonalNoteStore::read(personal.path(), created.id())?;
    assert_eq!(read, created);

    Ok(())
}

#[test]
fn list_is_empty_for_a_missing_knowledge_directory() -> Result<(), Box<dyn std::error::Error>> {
    let personal = TempDir::new("missing-dir")?;
    let notes = PersonalNoteStore::list(personal.path())?;
    assert!(notes.is_empty());
    Ok(())
}

#[test]
fn read_rejects_an_unknown_id() -> Result<(), Box<dyn std::error::Error>> {
    let personal = TempDir::new("unknown-id")?;
    fs::create_dir_all(personal.path().join("knowledge"))?;
    assert!(PersonalNoteStore::read(personal.path(), "note_missing").is_err());
    Ok(())
}

#[test]
fn promote_without_confirmation_writes_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let personal = TempDir::new("no-confirm-personal")?;
    let repository = TempDir::new("no-confirm-repo")?;

    let note = PersonalNoteStore::create(personal.path(), "Deployment steps")?;
    let diff = promote(&note, repository.path())?;
    assert!(!diff.destination_path().exists());

    let result = promote_confirmed(&note, repository.path(), false, false);
    assert!(result.is_err());
    assert!(!diff.destination_path().exists());
    assert!(fs::read_dir(repository.path())?.next().is_none());

    Ok(())
}

#[test]
fn promote_with_confirmation_writes_exact_expected_bytes() -> Result<(), Box<dyn std::error::Error>>
{
    let personal = TempDir::new("confirm-personal")?;
    let repository = TempDir::new("confirm-repo")?;

    let note = PersonalNoteStore::create(personal.path(), "Deployment steps")?;
    let diff = promote(&note, repository.path())?;
    assert!(!diff.would_overwrite());
    assert!(diff.unified_diff().contains("+++ b/"));

    let written = promote_confirmed(&note, repository.path(), true, false)?;
    assert_eq!(written, diff.destination_path());
    assert_eq!(
        written,
        repository
            .path()
            .join(".pebble")
            .join("knowledge")
            .join("deployment-steps.md")
    );

    let bytes_on_disk = fs::read(&written)?;
    assert_eq!(bytes_on_disk, note.content().as_bytes());

    Ok(())
}

#[test]
fn promote_rejects_path_traversal_titles() -> Result<(), Box<dyn std::error::Error>> {
    let personal = TempDir::new("traversal-personal")?;
    let repository = TempDir::new("traversal-repo")?;

    let note = PersonalNoteStore::create(personal.path(), "../../etc/passwd")?;
    assert!(promote(&note, repository.path()).is_err());
    assert!(promote_confirmed(&note, repository.path(), true, true).is_err());
    assert!(fs::read_dir(repository.path())?.next().is_none());

    Ok(())
}

#[test]
fn promote_requires_explicit_overwrite_acknowledgement() -> Result<(), Box<dyn std::error::Error>> {
    let personal = TempDir::new("overwrite-personal")?;
    let repository = TempDir::new("overwrite-repo")?;

    let first = PersonalNoteStore::create(personal.path(), "Shared title")?;
    let first_destination = promote_confirmed(&first, repository.path(), true, false)?;
    let original_bytes = fs::read(&first_destination)?;

    let second = PersonalNoteStore::create(personal.path(), "Shared title")?;
    assert_ne!(first.content(), second.content());

    let diff = promote(&second, repository.path())?;
    assert!(diff.would_overwrite());

    let rejected = promote_confirmed(&second, repository.path(), true, false);
    assert!(rejected.is_err());
    assert_eq!(fs::read(&first_destination)?, original_bytes);

    let accepted = promote_confirmed(&second, repository.path(), true, true)?;
    assert_eq!(accepted, first_destination);
    assert_eq!(fs::read(&first_destination)?, second.content().as_bytes());

    Ok(())
}

#[test]
fn promote_rewriting_identical_content_does_not_require_acknowledgement()
-> Result<(), Box<dyn std::error::Error>> {
    let personal = TempDir::new("identical-personal")?;
    let repository = TempDir::new("identical-repo")?;

    let note = PersonalNoteStore::create(personal.path(), "Stable title")?;
    let destination = promote_confirmed(&note, repository.path(), true, false)?;

    let diff = promote(&note, repository.path())?;
    assert!(!diff.would_overwrite());

    let rewritten = promote_confirmed(&note, repository.path(), true, false)?;
    assert_eq!(rewritten, destination);

    Ok(())
}
