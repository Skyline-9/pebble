use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::domain::{Citation, WorktreeRevision};
#[test]
fn returns_only_the_exact_cited_snapshot_lines() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("exact-lines")?;
    let (service, citation) = fixture.indexed_citation()?;

    let result = service.read(citation)?;

    assert_eq!(result.content, "pub fn cited() {}");
    Ok(())
}

use crate::service::{PebbleService, ServiceError};

use crate::service::citation_race::{self, RacePoint};

#[test]
fn snapshot_revision_race_is_stale_evidence() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("snapshot-revision")?;
    let (service, citation) = fixture.indexed_citation()?;
    citation_race::inject(RacePoint::BeforeSnapshotOpen, |repository| {
        assert!(fs::write(repository.join("src/lib.rs"), "pub fn replacement() {}\n").is_ok());
    });

    assert!(matches!(
        service.read(citation),
        Err(ServiceError::StaleEvidence(_))
    ));
    Ok(())
}

#[test]
fn terminal_worktree_change_is_stale_evidence() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("terminal-change")?;
    let (service, citation) = fixture.indexed_citation()?;
    citation_race::inject(RacePoint::AfterSnapshotOpen, |repository| {
        assert!(fs::write(repository.join("src/lib.rs"), "pub fn replacement() {}\n").is_ok());
    });

    assert!(matches!(
        service.read(citation),
        Err(ServiceError::StaleEvidence(_))
    ));
    Ok(())
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    repository: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Result<Self, Box<dyn std::error::Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "pebble-citation-race-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        let repository = root.join("repository");
        fs::create_dir_all(&home)?;
        fs::create_dir(&repository)?;
        git(&repository, &["init", "-q"])?;
        git(
            &repository,
            &["config", "user.email", "test@example.invalid"],
        )?;
        git(&repository, &["config", "user.name", "Pebble Test"])?;
        fs::create_dir(repository.join("src"))?;
        fs::write(
            repository.join("src/lib.rs"),
            "pub fn cited() {}\npub fn uncited() {}\n",
        )?;
        git(&repository, &["add", "src/lib.rs"])?;
        git(&repository, &["commit", "-qm", "fixture"])?;
        Ok(Self {
            root,
            home,
            repository,
        })
    }

    fn indexed_citation(&self) -> Result<(PebbleService, Citation), Box<dyn std::error::Error>> {
        let service = PebbleService::open(&self.home)?;
        let initialized = service.initialize(&self.repository)?;
        service.register(&self.repository, false)?;
        git(&self.repository, &["add", ".pebble/pebble.toml"])?;
        git(&self.repository, &["commit", "-qm", "configure pebble"])?;
        let indexed = service.index(&self.repository)?;
        let revision = WorktreeRevision::clean(&indexed.revision)?;
        let citation = Citation::new(initialized.repository_id, revision, "src/lib.rs", 1, 1)?;
        Ok((service, citation))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn git(repository: &Path, arguments: &[&str]) -> std::io::Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .status()?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| std::io::Error::other("test Git command failed"))
}
