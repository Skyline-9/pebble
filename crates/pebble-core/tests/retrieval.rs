#![forbid(unsafe_code)]

//! Model-free retrieval integration tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::domain::GenerationId;
use pebble_core::index::RepositoryCompiler;
use pebble_core::repository::RepositoryConfig;
use pebble_core::retrieval::{RetrievalEngine, SearchRequest};
use rusqlite::Connection;

#[test]
fn combines_exact_lexical_filters_and_citations() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("candidates")?;
    fs::write(
        fixture.repository().join("src/parser.rs"),
        "pub fn parse_request() {\n    validate_input();\n}\n",
    )?;
    fs::write(
        fixture.repository().join("guide.md"),
        "The parse request workflow validates input.\n",
    )?;
    let reader = compile(&fixture)?;
    let engine = RetrievalEngine::new(&reader);

    let response = engine.search(
        SearchRequest::new("parse_request src/parser.rs validates")?
            .with_language("rust")?
            .with_path_prefix("src/")?,
    )?;

    assert!(!response.packet().items().is_empty());
    assert!(
        response
            .packet()
            .items()
            .iter()
            .all(|item| item.citation.path().starts_with("src/"))
    );
    assert!(
        response
            .packet()
            .items()
            .iter()
            .any(|item| item.citation.path() == "src/parser.rs")
    );
    let scorers = response.packet().items()[0]
        .score_explanations
        .iter()
        .map(|score| score.scorer.as_str())
        .collect::<Vec<_>>();
    assert!(scorers.contains(&"lexical") || scorers.contains(&"exact_symbol"));
    assert!(
        response
            .trace()
            .active_scorers()
            .contains(&"exact_path".to_owned())
    );
    assert!(
        response
            .trace()
            .active_scorers()
            .contains(&"exact_symbol".to_owned())
    );
    assert!(
        response
            .trace()
            .active_scorers()
            .contains(&"exact_identifier".to_owned())
    );
    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .any(|candidate| candidate.reason() == "metadata_filter")
    );
    assert_eq!(response.trace().generation(), reader.id().as_str());
    Ok(())
}

#[test]
fn expands_graph_neighbors_from_lexical_seeds() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("graph-neighbor")?;
    fs::write(
        fixture.repository().join("linked.txt"),
        "unique_anchor evidence.\n\nstructurally adjacent context without the query term.\n",
    )?;
    let reader = compile(&fixture)?;

    let response = RetrievalEngine::new(&reader).search(SearchRequest::new("unique_anchor")?)?;

    assert!(
        response
            .trace()
            .active_scorers()
            .contains(&"graph".to_owned())
    );
    assert!(
        response
            .packet()
            .items()
            .iter()
            .any(|item| item.content.contains("structurally adjacent"))
    );
    Ok(())
}

#[test]
fn honors_minimum_and_maximum_budgets_and_diversifies_sources()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("budgets")?;
    for index in 0..5 {
        fs::write(
            fixture.repository().join(format!("source{index}.txt")),
            format!(
                "shared evidence source {index}\n\n{}\n",
                "bounded paragraph content ".repeat(600)
            ),
        )?;
    }
    let reader = compile(&fixture)?;
    let engine = RetrievalEngine::new(&reader);

    for budget in [1_000, 32_000] {
        let response =
            engine.search(SearchRequest::new("shared evidence")?.with_budget_tokens(budget)?)?;
        assert!(response.estimated_tokens() <= budget);
        if budget == 32_000 {
            assert!(response.packet().items().len() >= 3);
        } else {
            assert!(!response.packet().items().is_empty());
        }
    }
    Ok(())
}

#[test]
fn reports_contradictory_evidence_without_synthesizing_an_answer()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("contradiction")?;
    fs::write(
        fixture.repository().join("enabled.md"),
        "The audit feature is enabled for production.\n",
    )?;
    fs::write(
        fixture.repository().join("disabled.md"),
        "The audit feature is disabled for production.\n",
    )?;
    let reader = compile(&fixture)?;

    let response =
        RetrievalEngine::new(&reader).search(SearchRequest::new("audit feature production")?)?;

    assert!(
        response
            .packet()
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "contradictory_evidence")
    );
    assert_eq!(response.packet().items().len(), 2);
    Ok(())
}

#[test]
fn emits_only_sqlite_resolved_content_after_lexical_content_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("sqlite-authoritative")?;
    fs::write(
        fixture.repository().join("authority.txt"),
        "lexical_only_needle old Tantivy body\n",
    )?;
    let reader = compile(&fixture)?;
    let graph = Connection::open(reader.graph_path())?;
    graph.execute(
        "UPDATE chunks SET content = ?1",
        ["SQLite authoritative replacement"],
    )?;
    drop(graph);

    let response =
        RetrievalEngine::new(&reader).search(SearchRequest::new("lexical_only_needle")?)?;

    assert!(!response.packet().items().is_empty());
    assert!(
        response
            .packet()
            .items()
            .iter()
            .all(|item| item.content == "SQLite authoritative replacement")
    );
    assert!(
        response
            .packet()
            .items()
            .iter()
            .all(|item| !item.content.contains("old Tantivy body"))
    );
    Ok(())
}

#[test]
fn backfills_three_sources_after_stale_and_empty_candidates()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("backfill")?;
    for name in ["a", "b", "c", "d", "e"] {
        fs::write(
            fixture.repository().join(format!("{name}.txt")),
            format!("backfill_evidence from {name}\n"),
        )?;
    }
    let reader = compile(&fixture)?;
    let graph = Connection::open(reader.graph_path())?;
    graph.execute(
        "UPDATE chunks SET content = '' WHERE file_id = (
             SELECT file_id FROM files WHERE path = 'a.txt'
         )",
        [],
    )?;
    graph.execute(
        "DELETE FROM chunks WHERE file_id = (
             SELECT file_id FROM files WHERE path = 'b.txt'
         )",
        [],
    )?;
    drop(graph);

    let response = RetrievalEngine::new(&reader)
        .search(SearchRequest::new("backfill_evidence")?.with_max_results(3)?)?;
    let paths = response
        .packet()
        .items()
        .iter()
        .map(|item| item.citation.path())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(response.packet().items().len(), 3);
    assert_eq!(paths.len(), 3);
    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .any(|candidate| candidate.reason() == "empty")
    );
    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .any(|candidate| candidate.reason() == "stale")
    );
    Ok(())
}

#[test]
fn exact_paths_include_the_complete_trimmed_query_and_packet_bytes_fit_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("complete-budget")?;
    let directory = fixture
        .repository()
        .join("a deliberately long directory name with whitespace");
    fs::create_dir_all(&directory)?;
    let relative = "a deliberately long directory name with whitespace/long evidence path.md";
    fs::write(
        fixture.repository().join(relative),
        format!(
            "unicode_budget_needle {}\n",
            "🪨 \"quoted\" \\ escaped evidence ".repeat(2_000)
        ),
    )?;
    let reader = compile(&fixture)?;

    let response = RetrievalEngine::new(&reader)
        .search(SearchRequest::new(format!("  {relative}  "))?.with_budget_tokens(1_000)?)?;
    let encoded = serde_json::to_vec(response.packet())?;

    assert!(
        response
            .trace()
            .active_scorers()
            .contains(&"exact_path".to_owned())
    );
    assert!(
        response
            .packet()
            .items()
            .iter()
            .any(|item| item.citation.path() == relative)
    );
    assert!(encoded.len() <= 1_000);
    assert!(usize::try_from(response.estimated_tokens()).unwrap_or(usize::MAX) >= encoded.len());
    Ok(())
}

#[test]
fn diversifies_symbols_within_one_source() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("symbol-diversity")?;
    fs::write(
        fixture.repository().join("src/symbols.rs"),
        concat!(
            "fn alpha_symbol() { shared_symbol_evidence(); }\n",
            "fn beta_symbol() { shared_symbol_evidence(); }\n",
            "fn gamma_symbol() { shared_symbol_evidence(); }\n",
            "fn delta_symbol() { shared_symbol_evidence(); }\n",
        ),
    )?;
    let reader = compile(&fixture)?;

    let response = RetrievalEngine::new(&reader).search(
        SearchRequest::new("shared_symbol_evidence")?
            .with_kinds(["symbol"])?
            .with_max_results(3)?,
    )?;
    let lines = response
        .packet()
        .items()
        .iter()
        .map(|item| item.citation.start_line())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(response.packet().items().len(), 3);
    assert_eq!(lines.len(), 3);
    Ok(())
}

#[test]
fn contradiction_detection_uses_token_boundaries_and_supported_negations()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("contradiction-boundaries")?;
    fs::write(
        fixture.repository().join("positive.md"),
        concat!(
            "Enabled auditing. The value is true. The service supports Linux. ",
            "Operators must rotate keys.\n",
        ),
    )?;
    fs::write(
        fixture.repository().join("negative.md"),
        concat!(
            "Disabled auditing. The value is false. The service does not support Linux. ",
            "Operators must not reuse keys.\n",
        ),
    )?;
    let reader = compile(&fixture)?;
    let response = RetrievalEngine::new(&reader)
        .search(SearchRequest::new("auditing value service operators")?)?;
    assert!(
        response
            .packet()
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "contradictory_evidence")
    );

    let harmless = Fixture::new("contradiction-substrings")?;
    fs::write(
        harmless.repository().join("harmless.md"),
        "The feature was reenabled and its disabledness metric was removed.\n",
    )?;
    let harmless_reader = compile(&harmless)?;
    let harmless_response = RetrievalEngine::new(&harmless_reader)
        .search(SearchRequest::new("reenabled disabledness")?)?;
    assert!(
        harmless_response
            .packet()
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.code != "contradictory_evidence")
    );
    Ok(())
}

#[test]
fn contradiction_detection_requires_the_same_assertion_topic()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("compatible-obligations")?;
    fs::write(
        fixture.repository().join("rotate.md"),
        "Operators must rotate keys.\n",
    )?;
    fs::write(
        fixture.repository().join("reuse.md"),
        "Operators must not reuse keys.\n",
    )?;
    let reader = compile(&fixture)?;

    let response =
        RetrievalEngine::new(&reader).search(SearchRequest::new("operators rotate reuse keys")?)?;

    assert!(
        response
            .packet()
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.code != "contradictory_evidence")
    );
    Ok(())
}

fn compile(
    fixture: &Fixture,
) -> Result<pebble_core::index::GenerationReader, Box<dyn std::error::Error>> {
    let config = RepositoryConfig::load(fixture.repository())?;
    Ok(RepositoryCompiler::new(fixture.indexes()).compile(
        fixture.repository(),
        &config,
        GenerationId::try_from("retrieval".to_owned())?,
    )?)
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
            "pebble-retrieval-{label}-{}-{sequence}",
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
                "repository_id = \"retrieval.repo\"\n",
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
