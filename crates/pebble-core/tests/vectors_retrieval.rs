#![forbid(unsafe_code)]

//! Optional vector-candidate retrieval integration tests.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::domain::GenerationId;
use pebble_core::index::RepositoryCompiler;
use pebble_core::repository::RepositoryConfig;
use pebble_core::retrieval::{QueryEmbedding, RetrievalEngine, SearchRequest};
use pebble_core::vectors::TextEmbedder;

const DIMENSION: usize = 3;
const STORAGE_VECTOR: [f32; DIMENSION] = [1.0, 0.0, 0.0];
const NETWORK_VECTOR: [f32; DIMENSION] = [0.0, 1.0, 0.0];
const NOISE_VECTOR: [f32; DIMENSION] = [0.0, 0.0, 1.0];
const STORAGE_TRIGGER: &str = "storage engine module";
const NETWORK_TRIGGER: &str = "connection transport code";
const QUERY_TEXT: &str = "efficient persistent data retention approach";

/// A deterministic fake embedder used only in tests. It recognizes a fixed
/// set of trigger phrases and maps each to an orthogonal basis vector, so
/// cosine similarity in the test fixtures is exact and easy to assert on.
struct FakeEmbedder {
    fingerprint: [u8; 32],
}

impl TextEmbedder for FakeEmbedder {
    fn embed_one(&self, text: &str) -> io::Result<Vec<f32>> {
        if text == QUERY_TEXT || text.contains(STORAGE_TRIGGER) {
            Ok(STORAGE_VECTOR.to_vec())
        } else if text.contains(NETWORK_TRIGGER) {
            Ok(NETWORK_VECTOR.to_vec())
        } else {
            Ok(NOISE_VECTOR.to_vec())
        }
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }

    fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
}

const fn embedder() -> FakeEmbedder {
    FakeEmbedder {
        fingerprint: [11_u8; 32],
    }
}

fn write_topic_fixtures(fixture: &Fixture) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(
        fixture.repository().join("src/storage_topic.rs"),
        "fn manage_storage_engine_module() {\n    // storage engine module logic\n}\n",
    )?;
    fs::write(
        fixture.repository().join("src/network_topic.rs"),
        "fn manage_connection_transport_code() {\n    // connection transport code logic\n}\n",
    )?;
    fs::write(
        fixture.repository().join("src/misc_topic.rs"),
        "fn manage_miscellaneous_utility_routine() {\n    // miscellaneous utility routine logic\n}\n",
    )?;
    Ok(())
}

#[test]
fn vector_candidates_surface_semantically_similar_evidence_without_shared_keywords()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("semantic-match")?;
    write_topic_fixtures(&fixture)?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let embedder = embedder();
    let reader = RepositoryCompiler::new(fixture.indexes()).compile_with_embedder(
        fixture.repository(),
        &config,
        generation("semantic")?,
        &embedder,
    )?;
    let engine = RetrievalEngine::new(&reader);

    for word in QUERY_TEXT.split_whitespace() {
        assert!(
            !reader
                .lexical()
                .search_text(word, 10)?
                .into_iter()
                .any(|hit| hit.path().starts_with("src/")),
            "query word {word:?} must not lexically match any fixture file"
        );
    }

    let baseline = engine.search(SearchRequest::new(QUERY_TEXT)?)?;
    assert!(baseline.packet().items().is_empty());
    assert!(
        !baseline
            .trace()
            .active_scorers()
            .contains(&"vectors".to_owned())
    );

    let query_embedding =
        QueryEmbedding::new(embedder.embed_one(QUERY_TEXT)?, embedder.fingerprint())?;
    let response =
        engine.search_with_embedding(SearchRequest::new(QUERY_TEXT)?, Some(&query_embedding))?;

    assert!(!response.packet().items().is_empty());
    assert_eq!(
        response.packet().items()[0].citation.path(),
        "src/storage_topic.rs"
    );
    assert!(
        response
            .trace()
            .active_scorers()
            .contains(&"vectors".to_owned())
    );
    Ok(())
}

#[test]
fn generation_built_without_an_embedder_activates_and_searches_correctly()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("model-free-regression")?;
    write_topic_fixtures(&fixture)?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let reader = RepositoryCompiler::new(fixture.indexes()).compile(
        fixture.repository(),
        &config,
        generation("model-free")?,
    )?;

    assert!(reader.vectors_paths().is_none());

    let engine = RetrievalEngine::new(&reader);
    let response = engine.search(SearchRequest::new("manage_storage_engine_module")?)?;
    assert!(!response.packet().items().is_empty());
    assert!(
        !response
            .trace()
            .active_scorers()
            .contains(&"vectors".to_owned())
    );
    Ok(())
}

#[test]
fn a_mismatched_query_fingerprint_falls_back_to_the_model_free_path()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("fingerprint-mismatch")?;
    write_topic_fixtures(&fixture)?;
    let config = RepositoryConfig::load(fixture.repository())?;
    let embedder = embedder();
    let reader = RepositoryCompiler::new(fixture.indexes()).compile_with_embedder(
        fixture.repository(),
        &config,
        generation("mismatch")?,
        &embedder,
    )?;
    let engine = RetrievalEngine::new(&reader);

    let baseline = engine.search(SearchRequest::new(QUERY_TEXT)?)?;
    let mismatched_embedding = QueryEmbedding::new(STORAGE_VECTOR.to_vec(), [200_u8; 32])?;
    let response = engine
        .search_with_embedding(SearchRequest::new(QUERY_TEXT)?, Some(&mismatched_embedding))?;

    assert_eq!(
        baseline.packet().items().len(),
        response.packet().items().len()
    );
    assert!(
        !response
            .trace()
            .active_scorers()
            .contains(&"vectors".to_owned())
    );
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
            "pebble-vectors-retrieval-{label}-{}-{sequence}",
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
                "repository_id = \"vectors.repo\"\n",
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

fn run_git(repository: &Path, arguments: &[&str]) -> io::Result<()> {
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
        Err(io::Error::other("test Git command failed"))
    }
}
