#![forbid(unsafe_code)]

//! Adversarial tests for repository configuration and process boundaries.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use pebble_core::repository::{RepositoryConfig, SystemGit};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-hardening-{label}-{}-{suffix}",
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

fn git(repository: &Path, arguments: &[&str]) -> std::io::Result<()> {
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

#[cfg(unix)]
fn executable_script(path: &Path, body: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)
}

#[cfg(unix)]
#[test]
fn closed_pipes_do_not_disable_the_child_deadline() -> Result<(), Box<dyn std::error::Error>> {
    let binaries = TempDir::new("closed-pipes-bin")?;
    let repository = TempDir::new("closed-pipes-repo")?;
    executable_script(
        &binaries.path().join("git"),
        "exec 1>&-\nexec 2>&-\nsleep 10",
    )?;
    let system_git =
        SystemGit::discover_in_with_limits(binaries.path(), Duration::from_millis(100), 128)?;

    let started = Instant::now();
    let error = system_git
        .revision(repository.path())
        .err()
        .ok_or_else(|| std::io::Error::other("hung child unexpectedly succeeded"))?;

    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[cfg(unix)]
#[test]
fn configuration_rejects_symlinked_directory_and_file_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let repository = TempDir::new("config-links-repo")?;
    let outside = TempDir::new("config-links-outside")?;
    git(repository.path(), &["init", "-q"])?;
    fs::write(
        outside.path().join("pebble.toml"),
        "schema=1\nrepository_id=\"outside\"\n",
    )?;
    symlink(outside.path(), repository.path().join(".pebble"))?;

    assert!(RepositoryConfig::load(repository.path()).is_err());
    assert!(RepositoryConfig::initialize(repository.path(), &SystemGit::discover()?).is_err());

    fs::remove_file(repository.path().join(".pebble"))?;
    fs::create_dir(repository.path().join(".pebble"))?;
    symlink(
        outside.path().join("pebble.toml"),
        repository.path().join(".pebble/pebble.toml"),
    )?;
    assert!(RepositoryConfig::load(repository.path()).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn initialization_never_writes_through_a_symlinked_config_directory()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let repository = TempDir::new("config-create-link-repo")?;
    let outside = TempDir::new("config-create-link-outside")?;
    git(repository.path(), &["init", "-q"])?;
    symlink(outside.path(), repository.path().join(".pebble"))?;

    assert!(RepositoryConfig::initialize(repository.path(), &SystemGit::discover()?).is_err());
    assert!(!outside.path().join("pebble.toml").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn worktree_fingerprint_separates_file_content_from_symlink_target()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    let repository = TempDir::new("cross-kind")?;
    git(repository.path(), &["init", "-q"])?;
    fs::write(repository.path().join("kind"), "base\n")?;
    git(repository.path(), &["add", "--", "kind"])?;
    git(
        repository.path(),
        &[
            "-c",
            "user.name=Pebble Test",
            "-c",
            "user.email=pebble@example.invalid",
            "commit",
            "-qm",
            "base",
        ],
    )?;
    let (content, digest) = (0_u64..1024)
        .map(u64::to_le_bytes)
        .map(|content| (content, *blake3::hash(&content).as_bytes()))
        .find(|(_, digest)| !digest.contains(&0) && !digest.contains(&b'/'))
        .ok_or("could not construct collision fixture")?;
    fs::write(repository.path().join("kind"), content)?;
    let system_git = SystemGit::discover()?;
    let regular = system_git.revision(repository.path())?;

    fs::remove_file(repository.path().join("kind"))?;
    symlink(
        PathBuf::from(std::ffi::OsString::from_vec(digest.to_vec())),
        repository.path().join("kind"),
    )?;
    let symbolic = system_git.revision(repository.path())?;

    assert_ne!(regular.dirty_digest(), symbolic.dirty_digest());
    Ok(())
}
