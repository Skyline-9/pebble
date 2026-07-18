#![forbid(unsafe_code)]

//! Model-free command-line contract tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    repository: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "pebble-cli-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        let repository = root.join("repository");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(repository.join("src"))?;
        git(&repository, &["init", "-q"])?;
        git(
            &repository,
            &["config", "user.email", "test@example.invalid"],
        )?;
        git(&repository, &["config", "user.name", "Pebble Test"])?;
        fs::write(
            repository.join("src/lib.rs"),
            "pub fn local_needle() -> &'static str {\n    \"needle-value\"\n}\n",
        )?;
        git(&repository, &["add", "src/lib.rs"])?;
        git(&repository, &["commit", "-qm", "fixture"])?;
        Ok(Self {
            root,
            home,
            repository,
        })
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
            .env("NO_PROXY", "*");
        command
    }

    fn run(&self, arguments: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
        Ok(self.command().args(arguments).output()?)
    }

    fn initialize(&self) -> Result<String, Box<dyn std::error::Error>> {
        let output = self.run(&["--json", "init", repository_arg(&self.repository)])?;
        assert_success_json(&output);
        let config = fs::read_to_string(self.repository.join(".pebble/pebble.toml"))?;
        Ok(config_value(&config, "repository_id")?.to_owned())
    }

    fn register(&self) -> Result<String, Box<dyn std::error::Error>> {
        let repository_id = self.initialize()?;
        let output = self.run(&["--json", "register", repository_arg(&self.repository)])?;
        assert_success_json(&output);
        Ok(repository_id)
    }

    fn index(&self) -> Result<String, Box<dyn std::error::Error>> {
        let repository_id = self.register()?;
        git(&self.repository, &["add", ".pebble/pebble.toml"])?;
        git(&self.repository, &["commit", "-qm", "configure pebble"])?;
        let output = self.run(&["--json", "index", repository_arg(&self.repository)])?;
        assert_success_json(&output);
        Ok(repository_id)
    }

    fn state_repository(&self, repository_id: &str) -> PathBuf {
        self.home.join(".pebble/v1/repos").join(repository_id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn help_lists_only_the_declared_commands() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let output = fixture.run(&["--help"])?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = text(&output.stdout);
    for command in [
        "init",
        "register",
        "index",
        "watch",
        "search",
        "read",
        "health",
        "traces",
        "rebuild",
        "model",
        "note",
        "update",
        "workspace",
        "personal",
        "serve",
    ] {
        assert!(help.contains(command), "missing command {command}: {help}");
    }
    for forbidden in ["embedding", "vector", "answer"] {
        assert!(
            !help
                .lines()
                .any(|line| line.trim_start().starts_with(&format!("{forbidden} "))),
            "unexpected command {forbidden}: {help}"
        );
    }
    Ok(())
}

#[test]
fn init_creates_portable_config_and_private_local_state() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    fs::create_dir(fixture.home.join(".pebble"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fixture.home.join(".pebble"),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    let output = fixture.run(&["--json", "init", repository_arg(&fixture.repository)])?;
    assert_success_json(&output);
    assert!(text(&output.stdout).contains("\"repository_id\""));
    assert!(fixture.repository.join(".pebble/pebble.toml").is_file());
    let state = fixture.home.join(".pebble/v1");
    assert!(state.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(state)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(fixture.home.join(".pebble"))?
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
    Ok(())
}

#[test]
fn register_persists_checkout_under_versioned_state() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = fixture.register()?;
    let registry = fs::read_to_string(fixture.home.join(".pebble/v1/registry.json"))?;
    assert!(registry.contains(&repository_id));
    assert!(registry.contains(fixture.repository.to_string_lossy().as_ref()));
    Ok(())
}

#[test]
fn registered_alternate_worktree_can_be_indexed() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = fixture.index()?;
    let alternate = fixture.root.join("alternate");
    let output = Command::new("git")
        .args(["clone", "-q"])
        .arg(&fixture.repository)
        .arg(&alternate)
        .output()?;
    assert!(output.status.success(), "{}", text(&output.stderr));

    let output = fixture.run(&[
        "--json",
        "register",
        repository_arg(&alternate),
        "--alternate-worktree",
    ])?;
    assert_success_json(&output);
    let output = fixture.run(&["--json", "index", repository_arg(&alternate)])?;
    assert_success_json(&output);
    assert!(text(&output.stdout).contains(&repository_id));
    fs::write(alternate.join("src/lib.rs"), "pub fn alternate_only() {}\n")?;
    let output = fixture.run(&["--json", "index", repository_arg(&fixture.repository)])?;
    assert_success_json(&output);
    let revision = git_output(&fixture.repository, &["rev-parse", "HEAD"])?;
    let output = fixture.run(&[
        "--json",
        "read",
        "--repository",
        &repository_id,
        "--revision",
        revision.trim(),
        "--path",
        "src/lib.rs",
        "--start-line",
        "1",
        "--end-line",
        "1",
    ])?;
    assert_success_json(&output);
    assert!(text(&output.stdout).contains("local_needle"));
    Ok(())
}

#[test]
fn index_search_read_traces_health_and_rebuild_are_integrated()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = fixture.index()?;
    let repository = fixture.state_repository(&repository_id);
    assert!(repository.join("generations/CURRENT").is_file());

    let output = fixture.run(&[
        "--json",
        "search",
        "local_needle",
        "--repository",
        &repository_id,
        "--budget",
        "1000",
        "--limit",
        "5",
    ])?;
    assert_success_json(&output);
    let packet = text(&output.stdout);
    assert!(packet.contains("\"items\""));
    assert!(packet.contains("\"src/lib.rs\""));
    assert!(packet.contains("needle-value"));

    let revision = git_output(&fixture.repository, &["rev-parse", "HEAD"])?;
    let output = fixture.run(&[
        "--json",
        "read",
        "--repository",
        &repository_id,
        "--revision",
        revision.trim(),
        "--path",
        "src/lib.rs",
        "--start-line",
        "1",
        "--end-line",
        "2",
    ])?;
    assert_success_json(&output);
    assert!(text(&output.stdout).contains("local_needle"));

    let output = fixture.run(&[
        "--json",
        "traces",
        "--repository",
        &repository_id,
        "--limit",
        "10",
    ])?;
    assert_success_json(&output);
    assert!(text(&output.stdout).contains("\"generation\""));

    let output = fixture.run(&["--json", "health", "--repository", &repository_id])?;
    assert_success_json(&output);
    assert!(text(&output.stdout).contains("\"healthy\":true"));

    fs::write(
        fixture.repository.join("src/lib.rs"),
        "pub fn rebuilt_needle() -> bool { true }\n",
    )?;
    let output = fixture.run(&["--json", "rebuild", repository_arg(&fixture.repository)])?;
    assert_success_json(&output);
    assert!(text(&output.stdout).contains("\"generation\""));
    assert!(fixture.repository.join(".pebble/pebble.toml").is_file());
    Ok(())
}

#[test]
fn watch_once_reconciles_through_the_real_watcher() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = fixture.register()?;
    let output = fixture.run(&[
        "--json",
        "watch",
        repository_arg(&fixture.repository),
        "--once",
    ])?;
    assert_success_json(&output);
    assert!(text(&output.stdout).contains("\"generation\""));
    assert!(
        fixture
            .state_repository(&repository_id)
            .join("generations/CURRENT")
            .is_file()
    );
    Ok(())
}

#[test]
fn invalid_budgets_are_usage_errors_with_clean_stdout() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    for budget in ["999", "32001", "not-a-number"] {
        let output = fixture.run(&[
            "--json",
            "search",
            "needle",
            "--repository",
            "local",
            "--budget",
            budget,
        ])?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn absent_and_corrupt_indexes_are_unavailable_not_operational_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = fixture.register()?;

    let output = fixture.run(&["--json", "search", "needle", "--repository", &repository_id])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("unavailable"));

    let output = fixture.run(&["--json", "health", "--repository", &repository_id])?;
    assert_eq!(output.status.code(), Some(1));
    assert_valid_json(&output.stdout);
    assert!(output.stderr.is_empty());
    assert!(text(&output.stdout).contains("\"healthy\":false"));

    let generations = fixture.state_repository(&repository_id).join("generations");
    fs::create_dir_all(&generations)?;
    fs::write(generations.join("CURRENT"), b"not valid/current\n")?;
    let output = fixture.run(&["--json", "health", "--repository", &repository_id])?;
    assert_eq!(output.status.code(), Some(1));
    assert_valid_json(&output.stdout);
    assert!(text(&output.stdout).contains("rebuild"));
    Ok(())
}

#[test]
fn stale_citation_read_exits_one_without_source_output() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = fixture.index()?;
    let revision = git_output(&fixture.repository, &["rev-parse", "HEAD"])?;
    fs::write(
        fixture.repository.join("src/lib.rs"),
        "pub fn changed_after_index() {}\n",
    )?;

    let output = fixture.run(&[
        "--json",
        "read",
        "--repository",
        &repository_id,
        "--revision",
        revision.trim(),
        "--path",
        "src/lib.rs",
        "--start-line",
        "1",
        "--end-line",
        "1",
    ])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("stale"));
    Ok(())
}

#[test]
fn human_output_stays_on_stdout_and_diagnostics_stay_on_stderr()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let output = fixture.run(&["init", repository_arg(&fixture.repository)])?;
    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let output = fixture.run(&["search", "needle", "--repository", "missing"])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn local_flow_succeeds_with_network_routes_disabled() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = fixture.index()?;
    let output = fixture.run(&[
        "--json",
        "search",
        "needle-value",
        "--repository",
        &repository_id,
    ])?;
    assert_success_json(&output);
    Ok(())
}

fn repository_arg(path: &Path) -> &str {
    path.to_str().unwrap_or("")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn assert_success_json(output: &Output) {
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        text(&output.stderr),
        text(&output.stdout)
    );
    assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
    assert_valid_json(&output.stdout);
}

fn assert_valid_json(bytes: &[u8]) {
    let value = text(bytes);
    let value = value.trim();
    assert!(
        (value.starts_with('{') && value.ends_with('}'))
            || (value.starts_with('[') && value.ends_with(']')),
        "not JSON: {value}"
    );
}

fn config_value<'a>(config: &'a str, key: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let prefix = format!("{key} = \"");
    let line = config
        .lines()
        .find(|line| line.starts_with(&prefix))
        .ok_or("missing config value")?;
    Ok(line
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix('"'))
        .ok_or("malformed config value")?)
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

fn git_output(repository: &Path, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(text(&output.stderr).into());
    }
    Ok(text(&output.stdout))
}
