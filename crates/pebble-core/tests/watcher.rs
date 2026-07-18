#![forbid(unsafe_code)]

//! Bounded watcher coalescing and reconciliation integration tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pebble_core::index::GenerationReader;
use pebble_core::repository::RepositoryConfig;
use pebble_core::watcher::WatchService;

const QUIET_WAIT: Duration = Duration::from_secs(5);

#[test]
fn coalesces_create_modify_remove_rename_and_atomic_save_burst()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("burst")?;
    let mut service = fixture.watch()?;

    fs::write(fixture.repository().join("created.txt"), "first\n")?;
    fs::write(fixture.repository().join("created.txt"), "second\n")?;
    fs::write(fixture.repository().join("removed.txt"), "removed\n")?;
    fs::remove_file(fixture.repository().join("removed.txt"))?;
    fs::write(fixture.repository().join("before.txt"), "renamed\n")?;
    fs::rename(
        fixture.repository().join("before.txt"),
        fixture.repository().join("after.txt"),
    )?;
    fs::write(fixture.repository().join(".atomic.tmp"), "atomic\n")?;
    fs::rename(
        fixture.repository().join(".atomic.tmp"),
        fixture.repository().join("atomic.txt"),
    )?;

    let job = service
        .recv_timeout(QUIET_WAIT)?
        .ok_or("watcher did not produce a revision job")?;
    assert!(job.changed_paths().contains(&PathBuf::from("created.txt")));
    assert!(job.changed_paths().contains(&PathBuf::from("removed.txt")));
    assert!(job.changed_paths().contains(&PathBuf::from("after.txt")));
    assert!(job.changed_paths().contains(&PathBuf::from("atomic.txt")));
    assert!(!job.full_scan());
    let second = service.recv_timeout(Duration::from_millis(400))?;
    assert!(second.is_none(), "unexpected second job: {second:?}");

    let reader = GenerationReader::open_current(fixture.indexes())?;
    assert_eq!(reader.id(), job.generation());
    assert_eq!(reader.lexical().search_text("second", 10)?.len(), 1);
    assert_eq!(reader.lexical().search_text("atomic", 10)?.len(), 1);
    assert!(reader.lexical().search_text("removed", 10)?.is_empty());
    service.shutdown()?;
    Ok(())
}

#[test]
fn ignores_internal_and_configured_paths() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new_with_exclude("ignored", "[\"ignored/**\"]")?;
    let mut service = fixture.watch()?;

    fs::create_dir(fixture.repository().join("ignored"))?;
    fs::write(
        fixture.repository().join("ignored/generated.txt"),
        "ignored\n",
    )?;
    fs::write(
        fixture.repository().join(".git/pebble-watcher-noise"),
        "ignored\n",
    )?;
    fs::create_dir_all(fixture.repository().join(".pebble/local"))?;
    fs::write(
        fixture.repository().join(".pebble/local/state"),
        "ignored\n",
    )?;

    let ignored = service.recv_timeout(Duration::from_millis(600))?;
    assert!(ignored.is_none(), "unexpected ignored job: {ignored:?}");
    service.shutdown()?;
    Ok(())
}

#[test]
fn directory_creation_reconciles_children_created_before_the_watch_is_installed()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("directory-create")?;
    let mut service = fixture.watch()?;

    fs::create_dir(fixture.repository().join("new"))?;
    fs::write(
        fixture.repository().join("new/immediate.txt"),
        "immediate child\n",
    )?;

    let job = service
        .recv_timeout(QUIET_WAIT)?
        .ok_or("directory creation did not produce a revision job")?;
    assert!(job.full_scan());
    let reader = GenerationReader::open_current(fixture.indexes())?;
    assert_eq!(
        reader.lexical().search_text("immediate child", 10)?.len(),
        1
    );
    service.shutdown()?;
    Ok(())
}

#[test]
fn explicit_reconciliation_builds_a_fresh_immutable_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("reconcile")?;
    fs::write(fixture.repository().join("evidence.txt"), "old evidence\n")?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let old = pebble_core::index::RepositoryCompiler::new(fixture.indexes())
        .compile_fresh(fixture.repository(), &config)?;
    let mut service = fixture.watch()?;

    fs::write(fixture.repository().join("evidence.txt"), "new evidence\n")?;
    service.request_reconciliation()?;
    let job = service
        .recv_timeout(QUIET_WAIT)?
        .ok_or("reconciliation did not produce a revision job")?;

    assert!(job.full_scan());
    assert_ne!(old.id(), job.generation());
    assert_eq!(old.lexical().search_text("old", 10)?.len(), 1);
    assert!(old.lexical().search_text("new", 10)?.is_empty());
    let current = GenerationReader::open_current(fixture.indexes())?;
    assert_eq!(current.id(), job.generation());
    assert_eq!(current.lexical().search_text("new", 10)?.len(), 1);
    service.shutdown()?;
    Ok(())
}

#[test]
fn shutdown_reports_an_unreceived_terminal_compile_error() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new("terminal-error")?;
    let mut service = fixture.watch()?;
    fs::rename(
        fixture.repository().join(".git"),
        fixture.repository().join("disabled-git"),
    )?;
    service.request_reconciliation()?;
    std::thread::sleep(Duration::from_secs(3));

    let error = service.shutdown();
    assert!(error.is_err(), "terminal error must survive");
    if let Err(error) = error {
        assert!(error.to_string().contains("generation build failed"));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlink_repository_roots() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("symlink-root")?;
    let linked = fixture.root.join("linked-repository");
    symlink(fixture.repository(), &linked)?;
    let config = RepositoryConfig::load(fixture.repository())?;

    assert!(WatchService::start(&linked, fixture.indexes(), config).is_err());
    Ok(())
}

#[test]
fn rejects_generation_root_equal_to_or_contained_by_repository()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("generation-contained")?;
    let config = RepositoryConfig::load(fixture.repository())?;

    assert!(
        WatchService::start(fixture.repository(), fixture.repository(), config.clone()).is_err()
    );
    let contained = fixture.repository().join("generated");
    fs::create_dir(&contained)?;
    assert!(WatchService::start(fixture.repository(), &contained, config).is_err());
    Ok(())
}

#[test]
fn rejects_repository_contained_by_generation_root() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("repository-contained")?;
    let config = RepositoryConfig::load(fixture.repository())?;

    assert!(WatchService::start(fixture.repository(), &fixture.root, config).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlink_resolved_root_containment() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("symlink-containment")?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let contained = fixture.repository().join("generated");
    fs::create_dir(&contained)?;
    let linked_repository = fixture.root.join("linked-repository");
    symlink(fixture.repository(), &linked_repository)?;
    let linked = linked_repository.join("generated");

    assert!(WatchService::start(fixture.repository(), &linked, config).is_err());
    Ok(())
}

struct Fixture {
    root: PathBuf,
    repository: PathBuf,
    indexes: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_exclude(label, "[]")
    }

    fn new_with_exclude(label: &str, exclude: &str) -> Result<Self, Box<dyn std::error::Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "pebble-watcher-{label}-{}-{sequence}",
            std::process::id()
        ));
        let repository = root.join("repository");
        let indexes = root.join("indexes");
        fs::create_dir_all(repository.join(".pebble"))?;
        fs::create_dir(&indexes)?;
        let repository = repository.canonicalize()?;
        let indexes = indexes.canonicalize()?;
        run_git(&repository, &["init", "-q"])?;
        run_git(
            &repository,
            &[
                "-c",
                "user.name=Pebble",
                "-c",
                "user.email=pebble@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        )?;
        fs::write(
            repository.join(".pebble/pebble.toml"),
            format!(
                "schema = 1\n\
                 repository_id = \"watcher.repo\"\n\
                 include = [\"**/*\"]\n\
                 exclude = {exclude}\n\n\
                 [language_overrides]\n"
            ),
        )?;
        Ok(Self {
            root,
            repository,
            indexes,
        })
    }

    fn repository(&self) -> &Path {
        &self.repository
    }

    fn indexes(&self) -> &Path {
        &self.indexes
    }

    fn watch(&self) -> Result<WatchService, Box<dyn std::error::Error>> {
        Ok(WatchService::start(
            self.repository(),
            self.indexes(),
            RepositoryConfig::load(self.repository())?,
        )?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_git(repository: &Path, arguments: &[&str]) -> std::io::Result<()> {
    let status = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(repository)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("test Git command failed"))
    }
}
