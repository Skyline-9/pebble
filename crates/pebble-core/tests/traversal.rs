#![forbid(unsafe_code)]

//! Deterministic repository traversal and snapshot tests.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::repository::{
    RepositoryConfig, RepositoryError, RepositorySnapshot, SkipReason, SourceFile, SystemGit,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-traversal-{label}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        run_git(&path, &["init", "-q"])?;
        run_git(
            &path,
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
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn config(
        &self,
        include: &[&str],
        exclude: &[&str],
    ) -> Result<RepositoryConfig, Box<dyn std::error::Error>> {
        fs::create_dir_all(self.path().join(".pebble"))?;
        fs::write(
            self.path().join(".pebble/pebble.toml"),
            format!(
                concat!(
                    "schema = 1\n",
                    "repository_id = \"traversal.repo\"\n",
                    "include = [{}]\n",
                    "exclude = [{}]\n",
                    "\n[language_overrides]\n",
                ),
                quoted(include),
                quoted(exclude)
            ),
        )?;
        Ok(RepositoryConfig::load(self.path())?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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

fn quoted(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn snapshot(
    fixture: &Fixture,
    config: &RepositoryConfig,
) -> Result<RepositorySnapshot, Box<dyn std::error::Error>> {
    Ok(RepositorySnapshot::open(
        fixture.path(),
        config,
        &SystemGit::discover()?,
    )?)
}

fn collect(
    snapshot: &mut RepositorySnapshot,
) -> Result<Vec<SourceFile>, Box<dyn std::error::Error>> {
    snapshot
        .by_ref()
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn paths(files: &[SourceFile]) -> Vec<&str> {
    files.iter().map(SourceFile::path).collect()
}

#[test]
fn nested_gitignores_and_explicit_rules_have_deterministic_precedence()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("rules")?;
    fs::write(fixture.path().join(".gitignore"), "*.tmp\n")?;
    fs::create_dir_all(fixture.path().join("nested"))?;
    fs::write(fixture.path().join("nested/.gitignore"), "!keep.tmp\n")?;
    fs::write(fixture.path().join("nested/keep.tmp"), "keep")?;
    fs::write(fixture.path().join("nested/drop.tmp"), "drop")?;
    fs::write(fixture.path().join("nested/keep.rs"), "keep")?;
    fs::write(fixture.path().join("nested/excluded.rs"), "exclude wins")?;
    fs::write(fixture.path().join("outside.rs"), "not included")?;
    let config = fixture.config(&["nested/**"], &["nested/excluded.rs", "nested/keep.tmp"])?;

    let mut snapshot = snapshot(&fixture, &config)?;
    let files = collect(&mut snapshot)?;

    assert_eq!(paths(&files), ["nested/.gitignore", "nested/keep.rs"]);
    Ok(())
}

#[test]
fn git_and_pebble_configuration_are_excluded_but_repository_notes_remain()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("internal")?;
    fs::create_dir_all(fixture.path().join(".pebble/knowledge"))?;
    fs::write(fixture.path().join(".pebble/local"), "private local state")?;
    fs::write(
        fixture.path().join(".pebble/knowledge/architecture.md"),
        "portable source",
    )?;
    fs::write(fixture.path().join("source.rs"), "fn source() {}")?;
    let config = fixture.config(&["**/*"], &[])?;

    let mut snapshot = snapshot(&fixture, &config)?;
    let files = collect(&mut snapshot)?;
    let actual = paths(&files);

    assert_eq!(actual, [".pebble/knowledge/architecture.md", "source.rs"]);
    assert!(actual.iter().all(|path| !path.starts_with(".git/")));
    assert!(!actual.contains(&".pebble/pebble.toml"));
    Ok(())
}

#[test]
fn worktree_mutation_after_open_fails_the_snapshot_at_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("mutation")?;
    fs::write(fixture.path().join("source.rs"), "captured bytes")?;
    let config = fixture.config(&["**/*.rs"], &[])?;
    let mut snapshot = snapshot(&fixture, &config)?;

    fs::write(fixture.path().join("source.rs"), "later bytes")?;
    let result = snapshot.by_ref().collect::<Result<Vec<_>, _>>();

    assert!(matches!(result, Err(RepositoryError::WorktreeChanged)));
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_without_following_repository_escapes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("symlink")?;
    let outside = Fixture::new("outside")?;
    fs::write(outside.path().join("secret.rs"), "secret")?;
    std::os::unix::fs::symlink(
        outside.path().join("secret.rs"),
        fixture.path().join("escape.rs"),
    )?;
    fs::write(fixture.path().join("safe.rs"), "safe")?;
    let config = fixture.config(&["**/*"], &[])?;

    let mut snapshot = snapshot(&fixture, &config)?;
    let files = collect(&mut snapshot)?;

    assert_eq!(paths(&files), ["safe.rs"]);
    assert!(snapshot.diagnostics().iter().any(|diagnostic| {
        diagnostic.path() == "escape.rs" && diagnostic.reason() == SkipReason::SymbolicLink
    }));
    Ok(())
}

#[test]
fn binary_and_invalid_utf8_files_become_nonfatal_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("encoding")?;
    fs::write(fixture.path().join("binary.dat"), b"prefix\0suffix")?;
    fs::write(fixture.path().join("invalid.rs"), [0xff, 0xfe])?;
    fs::write(fixture.path().join("valid.rs"), "fn valid() {}\n")?;
    let config = fixture.config(&["**/*"], &[])?;

    let mut snapshot = snapshot(&fixture, &config)?;
    let files = collect(&mut snapshot)?;

    assert_eq!(paths(&files), ["valid.rs"]);
    assert!(snapshot.diagnostics().iter().any(|diagnostic| {
        diagnostic.path() == "binary.dat" && diagnostic.reason() == SkipReason::Binary
    }));
    assert!(snapshot.diagnostics().iter().any(|diagnostic| {
        diagnostic.path() == "invalid.rs" && diagnostic.reason() == SkipReason::InvalidUtf8
    }));
    Ok(())
}

#[test]
fn paths_are_lexical_and_unchanged_bytes_keep_stable_ids_and_hashes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("stable")?;
    fs::create_dir(fixture.path().join("middle"))?;
    fs::write(fixture.path().join("z.rs"), "same bytes")?;
    fs::write(fixture.path().join("a.rs"), "first")?;
    fs::write(fixture.path().join("middle/b.rs"), "middle")?;
    let config = fixture.config(&["**/*.rs"], &[])?;

    let mut first = snapshot(&fixture, &config)?;
    let first_files = collect(&mut first)?;
    let mut second = snapshot(&fixture, &config)?;
    let second_files = collect(&mut second)?;

    assert_eq!(paths(&first_files), ["a.rs", "middle/b.rs", "z.rs"]);
    for (before, after) in first_files.iter().zip(&second_files) {
        assert_eq!(before.id(), after.id());
        assert_eq!(before.content_hash(), after.content_hash());
        assert_eq!(before.contents(), after.contents());
    }
    assert_eq!(first.revision(), second.revision());
    Ok(())
}

#[test]
fn files_above_the_hard_32_mib_limit_are_skipped_before_loading()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("oversized")?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(fixture.path().join("oversized.rs"))?;
    file.set_len(MAX_SOURCE_BYTES + 1)?;
    fs::write(fixture.path().join("small.rs"), "small")?;
    let config = fixture.config(&["**/*.rs"], &[])?;

    let mut snapshot = snapshot(&fixture, &config)?;
    let files = collect(&mut snapshot)?;

    assert_eq!(paths(&files), ["small.rs"]);
    assert!(snapshot.diagnostics().iter().any(|diagnostic| {
        diagnostic.path() == "oversized.rs" && diagnostic.reason() == SkipReason::TooLarge
    }));
    Ok(())
}
