#![forbid(unsafe_code)]

//! Restricted system-Git boundary and adversarial path tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pebble_core::repository::SystemGit;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-git-{label}-{}-{suffix}",
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

fn committed_repository() -> Result<TempDir, Box<dyn std::error::Error>> {
    let repository = TempDir::new("repository")?;
    git(repository.path(), &["init", "-q"])?;
    fs::write(repository.path().join("tracked.txt"), "base\n")?;
    git(repository.path(), &["add", "--", "tracked.txt"])?;
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
    Ok(repository)
}

#[test]
fn clean_and_dirty_revisions_are_content_sensitive() -> Result<(), Box<dyn std::error::Error>> {
    let repository = committed_repository()?;
    let system_git = SystemGit::discover()?;
    let clean = system_git.revision(repository.path())?;

    assert!(clean.dirty_digest().is_none());
    fs::write(repository.path().join("tracked.txt"), "changed\n")?;
    fs::write(repository.path().join("untracked.txt"), "new\n")?;
    let dirty = system_git.revision(repository.path())?;
    assert_eq!(dirty.base_oid(), clean.base_oid());
    assert!(dirty.dirty_digest().is_some());

    fs::write(repository.path().join("untracked.txt"), "different\n")?;
    let changed_again = system_git.revision(repository.path())?;
    assert_ne!(dirty.dirty_digest(), changed_again.dirty_digest());
    Ok(())
}

#[test]
fn changed_paths_report_renames_without_losing_either_path()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = committed_repository()?;
    fs::rename(
        repository.path().join("tracked.txt"),
        repository.path().join("renamed.txt"),
    )?;
    git(repository.path(), &["add", "--all"])?;

    let paths = SystemGit::discover()?.changed_paths(repository.path())?;

    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].status(), "R");
    assert_eq!(paths[0].previous_path(), Some("tracked.txt"));
    assert_eq!(paths[0].path(), "renamed.txt");
    Ok(())
}

#[test]
fn option_like_filenames_are_data_not_arguments() -> Result<(), Box<dyn std::error::Error>> {
    let repository = committed_repository()?;
    for name in ["--help", "-c", "--config-env=core.fsmonitor=ATTACK"] {
        fs::write(repository.path().join(name), name)?;
    }

    let system_git = SystemGit::discover()?;
    let paths = system_git.changed_paths(repository.path())?;
    let names = paths
        .iter()
        .map(pebble_core::repository::ChangedPath::path)
        .collect::<Vec<_>>();

    assert!(names.contains(&"--help"));
    assert!(names.contains(&"-c"));
    assert!(names.contains(&"--config-env=core.fsmonitor=ATTACK"));
    assert!(
        system_git
            .revision(repository.path())?
            .dirty_digest()
            .is_some()
    );
    Ok(())
}

#[test]
fn hostile_alias_configuration_cannot_replace_fixed_subcommands()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = committed_repository()?;
    fs::write(
        repository.path().join(".git/config"),
        concat!(
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n",
            "[alias]\n\tstatus = !echo ATTACKED\n\trev-parse = !echo ATTACKED\n",
        ),
    )?;

    let system_git = SystemGit::discover()?;
    let revision = system_git.revision(repository.path())?;

    assert_eq!(revision.base_oid().len(), 40);
    assert!(system_git.changed_paths(repository.path())?.is_empty());
    Ok(())
}

#[test]
fn hostile_local_core_worktree_cannot_redirect_repository_reads()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = committed_repository()?;
    let hostile_worktree = TempDir::new("hostile-worktree")?;
    fs::write(hostile_worktree.path().join("tracked.txt"), "base\n")?;
    git(
        repository.path(),
        &[
            "config",
            "--local",
            "core.worktree",
            &hostile_worktree.path().to_string_lossy(),
        ],
    )?;
    fs::write(repository.path().join("tracked.txt"), "real change\n")?;

    let system_git = SystemGit::discover()?;
    let paths = system_git.changed_paths(repository.path())?;

    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].path(), "tracked.txt");
    assert!(
        system_git
            .revision(repository.path())?
            .dirty_digest()
            .is_some()
    );
    Ok(())
}

#[test]
fn hostile_local_excludes_file_cannot_hide_untracked_files()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = committed_repository()?;
    let hostile = TempDir::new("hostile-excludes")?;
    let excludes = hostile.path().join("ignore-everything");
    fs::write(&excludes, "*\n")?;
    git(
        repository.path(),
        &[
            "config",
            "--local",
            "core.excludesFile",
            &excludes.to_string_lossy(),
        ],
    )?;
    fs::write(repository.path().join("must-be-visible.txt"), "untracked\n")?;

    let system_git = SystemGit::discover()?;
    let paths = system_git.changed_paths(repository.path())?;

    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].path(), "must-be-visible.txt");
    assert!(
        system_git
            .revision(repository.path())?
            .dirty_digest()
            .is_some()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn dirty_fingerprint_rejects_symlinked_parent_escape() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let repository = committed_repository()?;
    let outside = TempDir::new("outside-parent")?;
    fs::create_dir(repository.path().join("link"))?;
    fs::write(repository.path().join("link/secret"), "inside\n")?;
    git(repository.path(), &["add", "--", "link/secret"])?;
    git(
        repository.path(),
        &[
            "-c",
            "user.name=Pebble Test",
            "-c",
            "user.email=pebble@example.invalid",
            "commit",
            "-qm",
            "nested",
        ],
    )?;
    fs::remove_file(repository.path().join("link/secret"))?;
    fs::remove_dir(repository.path().join("link"))?;
    fs::write(outside.path().join("secret"), "outside secret\n")?;
    symlink(outside.path(), repository.path().join("link"))?;

    let error = SystemGit::discover()?
        .revision(repository.path())
        .err()
        .ok_or_else(|| std::io::Error::other("symlinked parent unexpectedly accepted"))?;

    assert!(error.to_string().contains("symlink"), "{error}");
    Ok(())
}

#[test]
fn unmerged_index_stages_have_a_deterministic_dirty_fingerprint()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = committed_repository()?;
    git(repository.path(), &["checkout", "-qb", "side"])?;
    fs::write(repository.path().join("tracked.txt"), "side\n")?;
    git(repository.path(), &["add", "--", "tracked.txt"])?;
    git(
        repository.path(),
        &[
            "-c",
            "user.name=Pebble Test",
            "-c",
            "user.email=pebble@example.invalid",
            "commit",
            "-qm",
            "side",
        ],
    )?;
    git(repository.path(), &["checkout", "-q", "master"])?;
    fs::write(repository.path().join("tracked.txt"), "main\n")?;
    git(repository.path(), &["add", "--", "tracked.txt"])?;
    git(
        repository.path(),
        &[
            "-c",
            "user.name=Pebble Test",
            "-c",
            "user.email=pebble@example.invalid",
            "commit",
            "-qm",
            "main",
        ],
    )?;
    let merge = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(repository.path())
        .args([
            "-c",
            "user.name=Pebble Test",
            "-c",
            "user.email=pebble@example.invalid",
            "merge",
            "--no-edit",
            "side",
        ])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()?;
    assert!(!merge.status.success());

    let system_git = SystemGit::discover()?;
    let first = system_git.revision(repository.path())?;
    let second = system_git.revision(repository.path())?;

    assert!(first.dirty_digest().is_some());
    assert_eq!(first, second);
    assert_eq!(
        system_git.changed_paths(repository.path())?[0].status(),
        "U"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn hostile_fsmonitor_configuration_is_disabled() -> Result<(), Box<dyn std::error::Error>> {
    let repository = committed_repository()?;
    let marker = repository.path().join("fsmonitor-ran");
    let monitor = repository.path().join("hostile-monitor");
    executable_script(
        &monitor,
        &format!("printf attacked > '{}'", marker.display()),
    )?;
    git(
        repository.path(),
        &["config", "core.fsmonitor", &monitor.to_string_lossy()],
    )?;

    let paths = SystemGit::discover()?.changed_paths(repository.path())?;

    assert!(paths.iter().any(|path| path.path() == "hostile-monitor"));
    assert!(!marker.exists());
    Ok(())
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
fn fake_git(directory: &Path, body: &str) -> std::io::Result<()> {
    executable_script(&directory.join("git"), body)
}

#[cfg(unix)]
#[test]
fn subprocess_environment_is_restricted() -> Result<(), Box<dyn std::error::Error>> {
    let binaries = TempDir::new("environment-bin")?;
    let repository = TempDir::new("environment-repo")?;
    fake_git(
        binaries.path(),
        concat!(
            "test \"$GIT_CONFIG_NOSYSTEM\" = 1 || exit 80\n",
            "test \"$GIT_CONFIG_GLOBAL\" = /dev/null || exit 81\n",
            "test -z \"$GIT_DIR\" || exit 82\n",
            "test -z \"$GIT_WORK_TREE\" || exit 83\n",
            "case \"$*\" in\n",
            "  *rev-parse*) echo 0123456789012345678901234567890123456789 ;;\n",
            "  *status*) exit 0 ;;\n",
            "  *) exit 84 ;;\n",
            "esac",
        ),
    )?;
    let system_git = SystemGit::discover_in(binaries.path())?;

    let revision = system_git.revision(repository.path())?;

    assert_eq!(
        revision.base_oid(),
        "0123456789012345678901234567890123456789"
    );
    assert!(revision.dirty_digest().is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn output_overflow_is_rejected_and_child_is_terminated() -> Result<(), Box<dyn std::error::Error>> {
    let binaries = TempDir::new("overflow-bin")?;
    let repository = TempDir::new("overflow-repo")?;
    fake_git(
        binaries.path(),
        "while :; do printf '0123456789abcdef'; done",
    )?;
    let system_git =
        SystemGit::discover_in_with_limits(binaries.path(), Duration::from_secs(2), 128)?;

    let error = system_git
        .revision(repository.path())
        .err()
        .ok_or_else(|| std::io::Error::other("overflow unexpectedly succeeded"))?;

    assert!(error.to_string().contains("output limit"), "{error}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn simultaneous_stdout_and_stderr_overflow_is_terminated() -> Result<(), Box<dyn std::error::Error>>
{
    let binaries = TempDir::new("dual-overflow-bin")?;
    let repository = TempDir::new("dual-overflow-repo")?;
    fake_git(
        binaries.path(),
        concat!(
            "(while :; do printf '0123456789abcdef'; done) &\n",
            "while :; do printf 'fedcba9876543210' >&2; done",
        ),
    )?;
    let system_git =
        SystemGit::discover_in_with_limits(binaries.path(), Duration::from_secs(2), 128)?;

    let started = std::time::Instant::now();
    let error = system_git
        .revision(repository.path())
        .err()
        .ok_or_else(|| std::io::Error::other("overflow unexpectedly succeeded"))?;

    assert!(error.to_string().contains("output limit"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[cfg(unix)]
#[test]
fn subprocess_timeout_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
    let binaries = TempDir::new("timeout-bin")?;
    let repository = TempDir::new("timeout-repo")?;
    fake_git(binaries.path(), "sleep 10")?;
    let system_git =
        SystemGit::discover_in_with_limits(binaries.path(), Duration::from_millis(100), 128)?;

    let started = std::time::Instant::now();
    let error = system_git
        .revision(repository.path())
        .err()
        .ok_or_else(|| std::io::Error::other("timeout unexpectedly succeeded"))?;

    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn discovery_rejects_missing_git() -> Result<(), Box<dyn std::error::Error>> {
    let empty_path = TempDir::new("missing")?;

    let error = SystemGit::discover_in(empty_path.path())
        .err()
        .ok_or_else(|| std::io::Error::other("missing Git unexpectedly discovered"))?;

    assert!(error.to_string().contains("Git executable"), "{error}");
    Ok(())
}
