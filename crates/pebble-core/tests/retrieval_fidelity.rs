#![forbid(unsafe_code)]

//! Fidelity regressions for authoritative cited retrieval.

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
fn symbol_evidence_contains_only_its_cited_utf8_crlf_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("symbol-lines")?;
    fs::write(
        fixture.repository().join("src/symbol_lines.rs"),
        concat!(
            "const OUTSIDE_BEFORE: &str = \"not cited\";\r\n",
            "fn cited_symbol() {\r\n",
            "    let marker = \"🪨\";\r\n",
            "}\r\n",
            "const OUTSIDE_AFTER: &str = \"also not cited\";\r\n",
        ),
    )?;
    let reader = compile(&fixture)?;

    let response = RetrievalEngine::new(&reader).search(
        SearchRequest::new("cited_symbol")?
            .with_kinds(["symbol"])?
            .with_max_results(1)?,
    )?;
    let item = response
        .packet()
        .items()
        .first()
        .ok_or("expected symbol evidence")?;

    assert_eq!(item.citation.start_line(), 2);
    assert_eq!(item.citation.end_line(), 4);
    assert_eq!(
        item.content,
        "fn cited_symbol() {\r\n    let marker = \"🪨\";\r\n}\r\n"
    );
    Ok(())
}

#[test]
fn drops_and_traces_all_authoritative_metadata_disagreements()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("authoritative-metadata")?;
    for (path, symbol) in [
        ("src/language.rs", "language_symbol"),
        ("src/name.rs", "name_symbol"),
        ("src/kind.rs", "kind_symbol"),
    ] {
        fs::write(
            fixture.repository().join(path),
            format!("fn {symbol}() {{ shared_authoritative_filter(); }}\n"),
        )?;
    }
    let reader = compile(&fixture)?;
    let graph = Connection::open(reader.graph_path())?;
    graph.execute(
        "UPDATE files SET language = 'python' WHERE path = 'src/language.rs'",
        [],
    )?;
    graph.execute(
        "UPDATE symbols SET name = 'renamed_symbol' WHERE file_id = (
             SELECT file_id FROM files WHERE path = 'src/name.rs'
         )",
        [],
    )?;
    graph.execute(
        "UPDATE entities SET entity_kind = 'chunk' WHERE entity_id = (
             SELECT symbol.symbol_id FROM symbols AS symbol
             JOIN files AS file ON file.file_id = symbol.file_id
             WHERE file.path = 'src/kind.rs'
         )",
        [],
    )?;
    drop(graph);

    let response = RetrievalEngine::new(&reader).search(
        SearchRequest::new("shared_authoritative_filter")?
            .with_language("rust")?
            .with_kinds(["symbol"])?,
    )?;

    assert!(response.packet().items().is_empty());
    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .filter(|candidate| candidate.reason() == "metadata_disagreement")
            .count()
            >= 3
    );
    Ok(())
}

#[test]
fn resolves_candidates_before_applying_authoritative_filters()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("authoritative-filter-order")?;
    fs::write(
        fixture.repository().join("src/filter_order.rs"),
        "fn filter_order_symbol() { filter_order_needle(); }\n",
    )?;
    let reader = compile(&fixture)?;
    let graph = Connection::open(reader.graph_path())?;
    graph.execute(
        "UPDATE files SET language = 'python' WHERE path = 'src/filter_order.rs'",
        [],
    )?;
    drop(graph);

    let response = RetrievalEngine::new(&reader).search(
        SearchRequest::new("filter_order_needle")?
            .with_language("python")?
            .with_kinds(["symbol"])?,
    )?;

    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .any(|candidate| candidate.reason() == "metadata_disagreement")
    );
    Ok(())
}

#[test]
fn traces_an_unresolved_exact_file_metadata_candidate() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("stale-exact-file")?;
    let relative = "src/exact_file.rs";
    fs::write(
        fixture.repository().join(relative),
        "fn exact_file() { exact_file_body(); }\n",
    )?;
    let reader = compile(&fixture)?;
    let graph = Connection::open(reader.graph_path())?;
    let file_id: String = graph.query_row(
        "SELECT file_id FROM files WHERE path = ?1",
        [relative],
        |row| row.get(0),
    )?;
    graph.execute_batch("PRAGMA foreign_keys = OFF;")?;
    graph.execute("DELETE FROM entities WHERE entity_id = ?1", [&file_id])?;
    drop(graph);

    let response = RetrievalEngine::new(&reader).search(SearchRequest::new(relative)?)?;

    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .any(|candidate| { candidate.entity_id() == file_id && candidate.reason() == "stale" })
    );
    Ok(())
}

#[test]
fn traces_an_unresolvable_exact_symbol_metadata_candidate() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new("unresolvable-exact-symbol")?;
    fs::write(
        fixture.repository().join("src/split_symbol.rs"),
        "fn split_symbol() {\n    first();\n\n    second();\n}\n",
    )?;
    let reader = compile(&fixture)?;

    let response = RetrievalEngine::new(&reader).search(
        SearchRequest::new("split_symbol")?
            .with_kinds(["symbol"])?
            .with_max_results(1)?,
    )?;

    assert!(response.packet().items().is_empty());
    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .any(|candidate| candidate.reason() == "unresolvable")
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
        GenerationId::try_from("retrieval-fidelity".to_owned())?,
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
            "pebble-retrieval-fidelity-{label}-{}-{sequence}",
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
