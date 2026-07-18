#![forbid(unsafe_code)]

//! Deterministic repository compiler integration tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::domain::GenerationId;
use pebble_core::index::{CompilerFault, GenerationReader, IndexError, RepositoryCompiler};
use pebble_core::repository::RepositoryConfig;

#[test]
fn compiles_snapshot_to_both_indexes_and_is_repeatable() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("repeat")?;
    fs::write(
        fixture.repository().join("src/lib.rs"),
        "pub fn stable_symbol() { helper_call(); }\n",
    )?;
    fs::write(
        fixture.repository().join("notes.txt"),
        "Repository-owned note about deterministic compilation.\n",
    )?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let compiler = RepositoryCompiler::new(fixture.indexes());

    let first = compiler.compile(fixture.repository(), &config, generation("first")?)?;
    let first_ids = first
        .lexical()
        .search_text("deterministic compilation", 10)?
        .into_iter()
        .map(|hit| hit.entity_id().to_owned())
        .collect::<Vec<_>>();
    let first_counts = first.graph().counts()?;
    let second = compiler.compile(fixture.repository(), &config, generation("second")?)?;
    let second_ids = second
        .lexical()
        .search_text("deterministic compilation", 10)?
        .into_iter()
        .map(|hit| hit.entity_id().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(first_ids, second_ids);
    assert_eq!(first_counts, second.graph().counts()?);
    assert_eq!(second.id().as_str(), "second");
    assert_eq!(
        second.lexical().document_count(),
        second.graph().counts()?.chunks() + second.graph().counts()?.symbols()
    );
    Ok(())
}

#[test]
fn parser_failure_is_isolated_to_one_file() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("parser")?;
    fs::write(
        fixture.repository().join("good.rs"),
        "fn searchable_symbol() {}\n",
    )?;
    fs::write(fixture.repository().join("broken.rs"), "fn broken( {\n")?;
    let config = RepositoryConfig::load(fixture.repository())?;

    let reader = RepositoryCompiler::new(fixture.indexes()).compile(
        fixture.repository(),
        &config,
        generation("parser")?,
    )?;

    assert!(
        !reader
            .lexical()
            .exact_symbol("searchable_symbol", 10)?
            .is_empty()
    );
    assert!(!reader.lexical().search_text("broken", 10)?.is_empty());
    assert_eq!(reader.graph().counts()?.diagnostics(), 1);
    Ok(())
}

#[test]
fn injected_fault_between_stores_preserves_previous_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("fault")?;
    fs::write(fixture.repository().join("notes.txt"), "stable evidence\n")?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let compiler = RepositoryCompiler::new(fixture.indexes());
    compiler.compile(fixture.repository(), &config, generation("stable")?)?;
    fs::write(
        fixture.repository().join("notes.txt"),
        "replacement evidence\n",
    )?;

    let result = compiler.compile_with_fault(
        fixture.repository(),
        &config,
        generation("interrupted")?,
        CompilerFault::AfterGraph,
    );
    assert!(matches!(result, Err(IndexError::InjectedFault(_))));

    let current = GenerationReader::open_current(fixture.indexes())?;
    assert_eq!(current.id().as_str(), "stable");
    assert_eq!(current.lexical().search_text("stable", 10)?.len(), 1);
    assert!(current.lexical().search_text("replacement", 10)?.is_empty());
    assert!(fixture.indexes().join("interrupted.building").is_dir());
    Ok(())
}

#[test]
fn rejects_equal_count_lexical_index_swaps() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("cross-index-swap")?;
    fs::write(fixture.repository().join("first.txt"), "first evidence\n")?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let compiler = RepositoryCompiler::new(fixture.indexes());
    drop(compiler.compile(fixture.repository(), &config, generation("first-index")?)?);
    fs::remove_file(fixture.repository().join("first.txt"))?;
    fs::write(fixture.repository().join("second.txt"), "second evidence\n")?;
    drop(compiler.compile(fixture.repository(), &config, generation("second-index")?)?);
    let first_lexical = fixture.indexes().join("first-index/lexical");
    fs::rename(
        &first_lexical,
        fixture.indexes().join("first-index/original-lexical"),
    )?;
    fs::rename(
        fixture.indexes().join("second-index/lexical"),
        &first_lexical,
    )?;

    assert!(matches!(
        GenerationReader::open(fixture.indexes(), generation("first-index")?),
        Err(IndexError::RebuildRequired(_))
    ));
    Ok(())
}

#[test]
fn failed_owned_build_is_typed_and_fresh_compilation_retries_safely()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("retry")?;
    fs::write(fixture.repository().join("notes.txt"), "retry evidence\n")?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let compiler = RepositoryCompiler::new(fixture.indexes());

    assert!(matches!(
        compiler.compile_with_fault(
            fixture.repository(),
            &config,
            generation("failed")?,
            CompilerFault::AfterGraph,
        ),
        Err(IndexError::InjectedFault(_))
    ));
    assert!(matches!(
        compiler.compile(fixture.repository(), &config, generation("failed")?),
        Err(IndexError::IncompleteBuild { .. })
    ));

    let reader = compiler.compile_fresh(fixture.repository(), &config)?;
    assert_ne!(reader.id().as_str(), "failed");
    assert_eq!(reader.lexical().search_text("retry", 10)?.len(), 1);
    assert!(fixture.indexes().join("failed.building").is_dir());
    Ok(())
}

fn generation(value: &str) -> Result<GenerationId, pebble_core::error::DomainError> {
    GenerationId::try_from(value.to_owned())
}

struct Fixture {
    root: PathBuf,
    repository: PathBuf,
    indexes: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Result<Self, Box<dyn std::error::Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "pebble-compiler-{label}-{}-{sequence}",
            std::process::id()
        ));
        let repository = root.join("repository");
        let indexes = root.join("indexes");
        fs::create_dir_all(repository.join("src"))?;
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
        fs::create_dir(repository.join(".pebble"))?;
        fs::write(
            repository.join(".pebble/pebble.toml"),
            concat!(
                "schema = 1\n",
                "repository_id = \"compiler.repo\"\n",
                "include = [\"**/*\"]\n",
                "exclude = []\n\n",
                "[language_overrides]\n",
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
