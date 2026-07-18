#![forbid(unsafe_code)]

//! Plan 2 deterministic, adversarial, and recovery acceptance.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    repository: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "pebble-plan2-e2e-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        let repository = root.join("repository");
        fs::create_dir_all(&home)?;
        copy_tree(fixture_source(), &repository)?;
        git(&repository, &["init", "-q"])?;
        git(
            &repository,
            &["config", "user.email", "plan2@example.invalid"],
        )?;
        git(&repository, &["config", "user.name", "Plan 2 Acceptance"])?;
        git(&repository, &["add", "."])?;
        git(&repository, &["commit", "-qm", "plan2 fixture"])?;
        Ok(Self {
            root,
            home,
            repository,
        })
    }

    fn run(&self, arguments: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
        Ok(self.command().args(arguments).output()?)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pebble"));
        command
            .current_dir(&self.repository)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "");
        command
    }

    fn json(&self, arguments: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
        let output = self.run(arguments)?;
        assert!(
            output.status.success(),
            "command {arguments:?}: stderr={} stdout={}",
            text(&output.stderr),
            text(&output.stdout)
        );
        assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    fn initialize_and_index(&self) -> Result<(String, Value), Box<dyn std::error::Error>> {
        let initialized = self.json(&["--json", "init", repository_arg(&self.repository)])?;
        let repository_id = initialized["repository_id"]
            .as_str()
            .ok_or("missing repository id")?
            .to_owned();
        git(&self.repository, &["add", ".pebble/pebble.toml"])?;
        git(&self.repository, &["commit", "-qm", "configure pebble"])?;
        self.json(&["--json", "register", repository_arg(&self.repository)])?;
        let indexed = self.json(&["--json", "index", repository_arg(&self.repository)])?;
        Ok((repository_id, indexed))
    }

    fn search(
        &self,
        repository_id: &str,
        query: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        self.json(&[
            "--json",
            "search",
            query,
            "--repository",
            repository_id,
            "--limit",
            "10",
            "--budget",
            "6000",
        ])
    }

    fn generations(&self, repository_id: &str) -> PathBuf {
        self.home
            .join(".pebble/v1/repos")
            .join(repository_id)
            .join("generations")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn plan2_end_to_end_is_deterministic_cited_and_recoverable()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    assert_fixture_contract(&fixture.repository)?;
    let (repository_id, indexed) = fixture.initialize_and_index()?;
    assert_eq!(indexed["counts"]["files"].as_u64(), Some(19));
    assert!(indexed["revision"].as_str().is_some());

    let first = fixture.search(&repository_id, "RustExactChronometer")?;
    assert_packet_resolves(&fixture, &first)?;
    let first_id = packet_id(&first)?;
    let replay = fixture.search(&repository_id, "RustExactChronometer")?;
    assert_eq!(first_id, packet_id(&replay)?);
    assert_eq!(first, replay);
    assert_all_adjudicated_queries_resolve(&fixture, &repository_id)?;

    fs::write(
        fixture.repository.join("dirty-note.txt"),
        "DirtyWorktreeSentinel remains available without committing.\n",
    )?;
    let dirty_index = fixture.json(&["--json", "index", repository_arg(&fixture.repository)])?;
    assert!(
        dirty_index["revision"]
            .as_str()
            .is_some_and(|revision| revision.contains("+dirty."))
    );
    let dirty = fixture.search(&repository_id, "DirtyWorktreeSentinel")?;
    assert_packet_resolves(&fixture, &dirty)?;

    fs::remove_file(fixture.repository.join("dirty-note.txt"))?;
    let rebuilt = fixture.json(&["--json", "rebuild", repository_arg(&fixture.repository)])?;
    assert!(
        !rebuilt["revision"]
            .as_str()
            .unwrap_or_default()
            .contains("+dirty.")
    );
    let before_crash = fixture.search(&repository_id, "RustExactChronometer")?;

    let generations = fixture.generations(&repository_id);
    fs::create_dir(generations.join("CRASH-POINT.building"))?;
    fs::write(
        generations.join("CRASH-POINT.building/partial"),
        "disposable partial generation",
    )?;
    fs::write(generations.join("CURRENT.tmp"), "CRASH-POINT\n")?;
    let after_crash = fixture.search(&repository_id, "RustExactChronometer")?;
    assert_eq!(packet_id(&before_crash)?, packet_id(&after_crash)?);

    fs::write(generations.join("CURRENT"), "../malicious\n")?;
    let unavailable = fixture.run(&[
        "--json",
        "search",
        "RustExactChronometer",
        "--repository",
        &repository_id,
    ])?;
    assert_eq!(unavailable.status.code(), Some(1));
    assert!(unavailable.stdout.is_empty());
    assert!(text(&unavailable.stderr).contains("unavailable"));

    fixture.json(&["--json", "rebuild", repository_arg(&fixture.repository)])?;
    let recovered = fixture.search(&repository_id, "RustExactChronometer")?;
    assert_packet_resolves(&fixture, &recovered)?;
    assert_eq!(packet_id(&before_crash)?, packet_id(&recovered)?);
    Ok(())
}

fn assert_all_adjudicated_queries_resolve(
    fixture: &Fixture,
    repository_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest: Value =
        serde_json::from_slice(&fs::read(fixture.repository.join("acceptance.json"))?)?;
    for adjudication in manifest["queries"]
        .as_array()
        .ok_or("missing adjudicated queries")?
    {
        let packet = fixture.search(
            repository_id,
            adjudication["query"].as_str().ok_or("missing query")?,
        )?;
        assert_packet_resolves(fixture, &packet)?;
        let paths = packet["items"]
            .as_array()
            .ok_or("missing packet items")?
            .iter()
            .filter_map(|item| item["citation"]["path"].as_str())
            .collect::<Vec<_>>();
        for relevant in adjudication["relevance"]
            .as_object()
            .ok_or("missing relevance adjudication")?
            .keys()
        {
            assert!(
                paths.contains(&relevant.as_str()),
                "{} did not retrieve {relevant}: {paths:?}",
                adjudication["id"]
            );
        }
    }
    Ok(())
}

fn assert_fixture_contract(repository: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let manifest: Value = serde_json::from_slice(&fs::read(repository.join("acceptance.json"))?)?;
    assert_eq!(
        manifest["languages"].as_array().map(Vec::len),
        Some(14),
        "fixture must cover every packaged language mode"
    );
    assert!(
        manifest["queries"]
            .as_array()
            .is_some_and(|queries| queries.len() >= 30)
    );
    for required in [
        "unknown",
        "scip",
        "dirty_git",
        "malicious_filenames",
        "distractors",
        "exact_symbol_queries",
        "crash_points",
    ] {
        assert_eq!(manifest["coverage"][required], true, "missing {required}");
    }
    assert!(repository.join("index.scip").is_file());
    assert!(repository.join("--hosted-model-endpoint.txt").is_file());
    assert!(repository.join("semi;curl-never-runs.txt").is_file());
    Ok(())
}

fn assert_packet_resolves(
    fixture: &Fixture,
    packet: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let items = packet["items"].as_array().ok_or("missing packet items")?;
    assert!(!items.is_empty());
    for item in items {
        let citation = &item["citation"];
        let revision = citation_revision(&citation["revision"])?;
        let path = format!(
            "--path={}",
            citation["path"].as_str().ok_or("missing path")?
        );
        let output = fixture.run(&[
            "--json",
            "read",
            "--repository",
            citation["repository"]
                .as_str()
                .ok_or("missing repository")?,
            "--revision",
            &revision,
            &path,
            "--start-line",
            &citation["start_line"]
                .as_u64()
                .ok_or("missing start")?
                .to_string(),
            "--end-line",
            &citation["end_line"]
                .as_u64()
                .ok_or("missing end")?
                .to_string(),
        ])?;
        assert!(
            output.status.success(),
            "citation failed: {}",
            text(&output.stderr)
        );
        let resolved: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(
            resolved["content"].as_str().map(str::trim_end),
            item["content"].as_str().map(str::trim_end)
        );
    }
    Ok(())
}

fn citation_revision(revision: &Value) -> Result<String, Box<dyn std::error::Error>> {
    let base = revision["base_oid"].as_str().ok_or("missing base OID")?;
    Ok(revision["dirty_digest"].as_str().map_or_else(
        || base.to_owned(),
        |digest| format!("{base}+dirty.{digest}"),
    ))
}

fn packet_id(packet: &Value) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = serde_json::to_vec(packet)?;
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("missing hash stdin")?
        .write_all(&bytes)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err("git hash-object failed".into());
    }
    Ok(text(&output.stdout).trim().to_owned())
}

fn fixture_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/plan2")
}

fn copy_tree(source: PathBuf, target: &Path) -> std::io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn git(repository: &Path, arguments: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(text(&output.stderr).into());
    }
    Ok(())
}

fn repository_arg(path: &Path) -> &str {
    path.to_str().unwrap_or("")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
