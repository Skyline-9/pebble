#![forbid(unsafe_code)]

//! Adversarial bounds and trace tests for retrieval.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::domain::GenerationId;
use pebble_core::index::{GenerationReader, RepositoryCompiler};
use pebble_core::repository::RepositoryConfig;
use pebble_core::retrieval::{RetrievalEngine, SearchRequest};

#[test]
fn truncates_at_paragraph_boundaries_and_traces_omissions() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new("truncate")?;
    for index in 0..8 {
        fs::write(
            fixture.repository().join(format!("evidence{index}.md")),
            format!(
                "bounded retrieval evidence {index}.\n\n{}\n\ntrailing marker {index}.\n",
                "whole paragraph sentence. ".repeat(900)
            ),
        )?;
    }
    let reader = compile(&fixture)?;
    let response = RetrievalEngine::new(&reader)
        .search(SearchRequest::new("bounded retrieval evidence")?.with_budget_tokens(1_000)?)?;

    assert!(response.estimated_tokens() <= 1_000);
    assert!(
        response
            .packet()
            .items()
            .iter()
            .all(|item| !item.content.ends_with("whole paragraph sent"))
    );
    assert!(!response.trace().omitted_candidates().is_empty());
    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .any(|candidate| candidate.reason() == "budget")
    );
    Ok(())
}

#[test]
fn truncates_code_at_a_syntax_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("syntax")?;
    fs::write(
        fixture.repository().join("bounded.rs"),
        format!(
            "fn bounded() {{ syntax_boundary_needle(); {} }}\n",
            "statement(); ".repeat(1_500)
        ),
    )?;
    let reader = compile(&fixture)?;

    let response = RetrievalEngine::new(&reader)
        .search(SearchRequest::new("syntax_boundary_needle")?.with_budget_tokens(1_000)?)?;

    assert!(!response.packet().items().is_empty());
    assert!(
        response
            .packet()
            .items()
            .iter()
            .all(|item| item.content.ends_with([';', '}']))
    );
    Ok(())
}

#[test]
fn local_trace_is_bounded_jsonl_and_rejects_insecure_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("trace")?;
    fs::write(
        fixture.repository().join("evidence.txt"),
        "private needle\n",
    )?;
    let reader = compile(&fixture)?;
    let trace = fixture.root.join("traces/query.jsonl");
    let engine = RetrievalEngine::new(&reader);

    engine.search(SearchRequest::new("private needle")?.with_trace_path(&trace)?)?;
    let line = fs::read_to_string(&trace)?;
    let value: serde_json::Value = serde_json::from_str(line.trim())?;
    assert!(value.get("query").is_none());
    assert!(!line.contains("private needle"));
    assert!(line.len() <= 64 * 1024);

    let linked = fixture.root.join("linked-trace.jsonl");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&trace, &linked)?;
        assert!(
            SearchRequest::new("needle")?
                .with_trace_path(&linked)
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn concurrent_trace_appends_keep_complete_jsonl_records() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new("concurrent-trace")?;
    fs::write(
        fixture.repository().join("evidence.txt"),
        "concurrent trace needle\n",
    )?;
    drop(compile(&fixture)?);
    let trace = fixture.root.join("traces/concurrent.jsonl");
    let indexes = fixture.indexes().to_owned();
    let readers = (0..8)
        .map(|_| GenerationReader::open_current(&indexes))
        .collect::<Result<Vec<_>, _>>()?;
    let handles = readers
        .into_iter()
        .map(|reader| {
            let trace = trace.clone();
            std::thread::spawn(move || -> Result<(), String> {
                let request = SearchRequest::new("concurrent trace needle")
                    .and_then(|request| request.with_trace_path(&trace))
                    .map_err(|error| error.to_string())?;
                RetrievalEngine::new(&reader)
                    .search(request)
                    .map_err(|error| error.to_string())?;
                Ok(())
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle
            .join()
            .map_err(|_| std::io::Error::other("trace thread panicked"))?
            .map_err(std::io::Error::other)?;
    }
    let lines = fs::read_to_string(trace)?;
    assert_eq!(lines.lines().count(), 8);
    for line in lines.lines() {
        serde_json::from_str::<serde_json::Value>(line)?;
    }
    Ok(())
}

#[test]
fn independent_processes_append_complete_trace_records() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("process-trace")?;
    fs::write(
        fixture.repository().join("evidence.txt"),
        "multiprocess trace needle\n",
    )?;
    drop(compile(&fixture)?);
    let trace = fixture.root.join("traces/process.jsonl");
    let barrier = fixture.root.join("trace-start");
    let mut workers = Vec::new();
    for index in 0..8 {
        let ready = fixture.root.join(format!("trace-ready-{index}"));
        workers.push(trace_worker(fixture.indexes(), &trace, &barrier, &ready)?);
        wait_for(&ready)?;
    }
    fs::write(&barrier, "go")?;
    for worker in &mut workers {
        assert!(worker.wait()?.success());
    }

    let lines = fs::read_to_string(trace)?;
    assert_eq!(lines.lines().count(), 8);
    for line in lines.lines() {
        serde_json::from_str::<serde_json::Value>(line)?;
    }
    Ok(())
}

#[test]
fn retrieval_trace_process_worker() -> Result<(), Box<dyn std::error::Error>> {
    let Some(indexes) = std::env::var_os("PEBBLE_TRACE_INDEXES") else {
        return Ok(());
    };
    let trace = PathBuf::from(std::env::var_os("PEBBLE_TRACE_PATH").ok_or("missing trace path")?);
    let barrier =
        PathBuf::from(std::env::var_os("PEBBLE_TRACE_BARRIER").ok_or("missing trace barrier")?);
    let ready = PathBuf::from(std::env::var_os("PEBBLE_TRACE_READY").ok_or("missing ready path")?);
    let reader = GenerationReader::open_current(Path::new(&indexes))?;
    fs::write(ready, "ready")?;
    wait_for(&barrier)?;
    RetrievalEngine::new(&reader)
        .search(SearchRequest::new("multiprocess trace needle")?.with_trace_path(&trace)?)?;
    Ok(())
}

#[test]
fn traces_every_omission_at_the_global_candidate_maximum() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new("candidate-maximum")?;
    for index in 0..128 {
        fs::write(
            fixture
                .repository()
                .join(format!("candidate-{index:03}.txt")),
            format!("global_candidate_needle evidence {index}\n"),
        )?;
    }
    let reader = compile(&fixture)?;
    let trace_path = fixture.root.join("traces/maximum.jsonl");
    let response = RetrievalEngine::new(&reader).search(
        SearchRequest::new("global_candidate_needle")?
            .with_max_results(1)?
            .with_budget_tokens(1_000)?
            .with_trace_path(&trace_path)?,
    )?;

    assert_eq!(response.trace().selected_candidates().len(), 1);
    assert_eq!(response.trace().omitted_candidates().len(), 255);
    assert!(
        response
            .trace()
            .omitted_candidates()
            .iter()
            .any(|candidate| candidate.reason() == "unresolvable")
    );
    assert!(
        response
            .trace()
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic != "omitted_candidates_truncated")
    );
    let trace_record = fs::read_to_string(trace_path)?;
    assert!(trace_record.len() <= 64 * 1024);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(trace_record.trim())?["omitted_candidates"]
            .as_array()
            .ok_or("omitted candidates were not encoded as an array")?
            .len(),
        255
    );
    Ok(())
}

#[test]
fn returns_an_explicit_error_when_the_global_candidate_bound_is_exceeded()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("candidate-overflow")?;
    for index in 0..129 {
        fs::write(
            fixture
                .repository()
                .join(format!("overflow-{index:03}.txt")),
            format!("global_overflow_needle evidence {index}\n"),
        )?;
    }
    let reader = compile(&fixture)?;
    let error = RetrievalEngine::new(&reader)
        .search(SearchRequest::new("global_overflow_needle")?)
        .err()
        .ok_or("expected a candidate overflow error")?;

    assert!(error.to_string().contains("candidate"));
    assert!(error.to_string().contains("bound"));
    Ok(())
}

#[test]
fn active_scorers_and_ties_are_deterministic() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("determinism")?;
    fs::write(fixture.repository().join("a.txt"), "equal ranking needle\n")?;
    fs::write(fixture.repository().join("b.txt"), "equal ranking needle\n")?;
    let reader = compile(&fixture)?;
    let engine = RetrievalEngine::new(&reader);

    let first = engine.search(SearchRequest::new("equal ranking needle")?)?;
    let second = engine.search(SearchRequest::new("equal ranking needle")?)?;
    let first_paths = first
        .packet()
        .items()
        .iter()
        .map(|item| item.citation.path())
        .collect::<Vec<_>>();
    let second_paths = second
        .packet()
        .items()
        .iter()
        .map(|item| item.citation.path())
        .collect::<Vec<_>>();
    assert_eq!(first_paths, second_paths);
    assert!(first.packet().items().iter().all(|item| {
        item.score_explanations
            .iter()
            .all(|score| score.scorer == "lexical")
    }));
    Ok(())
}

fn trace_worker(
    indexes: &Path,
    trace: &Path,
    barrier: &Path,
    ready: &Path,
) -> std::io::Result<Child> {
    Command::new(std::env::current_exe()?)
        .args(["--exact", "retrieval_trace_process_worker", "--nocapture"])
        .env("PEBBLE_TRACE_INDEXES", indexes)
        .env("PEBBLE_TRACE_PATH", trace)
        .env("PEBBLE_TRACE_BARRIER", barrier)
        .env("PEBBLE_TRACE_READY", ready)
        .stdout(Stdio::null())
        .spawn()
}

fn wait_for(path: &Path) -> std::io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !path.exists() {
        if std::time::Instant::now() >= deadline {
            return Err(std::io::Error::other("trace barrier was not created"));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Ok(())
}

fn compile(fixture: &Fixture) -> Result<GenerationReader, Box<dyn std::error::Error>> {
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
            "pebble-retrieval-adversarial-{label}-{}-{sequence}",
            std::process::id()
        ));
        let repository = root.join("repository");
        let indexes = root.join("indexes");
        fs::create_dir_all(&repository)?;
        fs::create_dir(&indexes)?;
        let root = root.canonicalize()?;
        let repository = root.join("repository");
        let indexes = root.join("indexes");
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
