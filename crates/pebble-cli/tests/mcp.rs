#![forbid(unsafe_code)]

//! Bounded stdio MCP contract tests.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[path = "mcp/execution.rs"]
mod execution;
#[path = "mcp/lifecycle.rs"]
mod lifecycle;
#[path = "mcp/schema.rs"]
mod schema;

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    repository: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "pebble-mcp-{}-{}",
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
            "pub fn mcp_needle() -> &'static str {\n    \"bounded-evidence\"\n}\n",
        )?;
        git(&repository, &["add", "src/lib.rs"])?;
        git(&repository, &["commit", "-qm", "fixture"])?;
        Ok(Self {
            root,
            home,
            repository,
        })
    }

    fn server(&self) -> Result<Server, Box<dyn std::error::Error>> {
        Server::spawn(&self.home, &self.repository)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Server {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Server {
    fn spawn(home: &Path, repository: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pebble"))
            .arg("serve")
            .current_dir(repository)
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "*")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take().ok_or("missing server stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("missing server stdout")?);
        Ok(Self {
            child,
            input,
            output,
        })
    }

    #[allow(clippy::needless_pass_by_value)]
    fn request(&mut self, message: Value) -> Result<Value, Box<dyn std::error::Error>> {
        let line = self.request_frame(message)?;
        Ok(serde_json::from_slice(&line)?)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn request_frame(&mut self, message: Value) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        serde_json::to_writer(&mut self.input, &message)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        let mut line = Vec::new();
        self.output.read_until(b'\n', &mut line)?;
        assert!(!line.is_empty(), "server closed without a response");
        Ok(line)
    }

    fn initialize(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        self.request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "pebble-test", "version": "1"}
            }
        }))
    }

    #[allow(clippy::needless_pass_by_value)]
    fn call(
        &mut self,
        id: u64,
        name: &str,
        arguments: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        self.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }))
    }

    fn close(mut self) -> Result<(std::process::ExitStatus, String), Box<dyn std::error::Error>> {
        drop(self.input);
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                let mut stderr = String::new();
                if let Some(mut stream) = self.child.stderr.take() {
                    std::io::Read::read_to_string(&mut stream, &mut stderr)?;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(3),
                    "EOF shutdown was not prompt"
                );
                return Ok((status, stderr));
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "server did not stop on EOF"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn initialize_and_tools_list_expose_exact_bounded_schemas() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let mut server = fixture.server()?;
    let initialized = server.initialize()?;
    assert_eq!(initialized["result"]["serverInfo"]["name"], "pebble");
    assert!(initialized["result"]["capabilities"]["tools"].is_object());
    let listed = server.request(json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
    }))?;
    let tools = listed["result"]["tools"]
        .as_array()
        .ok_or("missing tools")?;
    let names = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "repository_init",
            "repository_register",
            "repository_index",
            "search",
            "evidence_read",
            "index_health",
            "trace_list",
            "projection_rebuild",
            "model_install",
            "model_list",
            "model_select",
            "model_remove",
            "note_list",
            "note_read",
            "update_list",
            "update_apply",
            "workspace_create",
            "workspace_add_repository",
            "workspace_list",
            "workspace_search",
            "personal_note_create",
            "personal_note_list",
            "personal_note_promote"
        ]
    );
    for tool in tools {
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema["properties"].is_object());
    }
    let search = tools
        .iter()
        .find(|tool| tool["name"] == "search")
        .ok_or("search")?;
    assert_eq!(
        search["inputSchema"]["required"],
        json!(["query", "repository"])
    );
    assert_eq!(
        search["inputSchema"]["properties"]["budget_tokens"]["minimum"],
        1000
    );
    assert_eq!(
        search["inputSchema"]["properties"]["budget_tokens"]["maximum"],
        32000
    );
    let read = tools
        .iter()
        .find(|tool| tool["name"] == "evidence_read")
        .ok_or("evidence_read")?;
    assert_eq!(
        read["inputSchema"]["required"],
        json!(["repository", "revision", "path", "start_line", "end_line"])
    );
    let (status, stderr) = server.close()?;
    assert!(status.success(), "{stderr}");
    Ok(())
}

#[test]
fn search_response_is_bounded_and_concurrent_calls_complete()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = initialize_with_cli(&fixture)?;
    let mut server = fixture.server()?;
    server.initialize()?;
    for id in 10..=25 {
        serde_json::to_writer(
            &mut server.input,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": "search",
                    "arguments": {
                        "query": "mcp_needle",
                        "repository": repository_id,
                        "budget_tokens": 1000
                    }
                }
            }),
        )?;
        server.input.write_all(b"\n")?;
    }
    server.input.flush()?;
    let mut ids = Vec::new();
    for _ in 10..=25 {
        let mut line = String::new();
        server.output.read_line(&mut line)?;
        let response: Value = serde_json::from_str(&line)?;
        ids.push(response["id"].as_u64().ok_or("response id")?);
        assert!(line.len() <= 1_000);
        assert_eq!(response["result"]["content"], json!([]));
        assert!(response["result"]["structuredContent"].is_object());
    }
    ids.sort_unstable();
    assert_eq!(ids, (10..=25).collect::<Vec<_>>());
    let (status, stderr) = server.close()?;
    assert!(status.success(), "{stderr}");
    Ok(())
}

#[test]
fn full_search_stdout_frame_fits_requested_budget() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = initialize_with_cli(&fixture)?;
    let mut server = fixture.server()?;
    server.initialize()?;
    let frame = server.request_frame(json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "tools/call",
        "params": {
            "name": "search",
            "arguments": {
                "query": "mcp_needle",
                "repository": repository_id,
                "budget_tokens": 1000
            }
        }
    }))?;
    assert!(frame.len() <= 1_000, "frame was {} bytes", frame.len());
    let response: Value = serde_json::from_slice(&frame)?;
    assert_eq!(response["result"]["content"], json!([]));
    assert!(response["result"]["structuredContent"]["items"].is_array());
    let (status, stderr) = server.close()?;
    assert!(status.success(), "{stderr}");
    Ok(())
}

#[test]
fn exact_maximum_frame_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut server = fixture.server()?;
    let prefix = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":""#;
    let ending = br#"","version":"1"}}}"#;
    let padding = 1024 * 1024 - prefix.len() - ending.len() - 1;
    server.input.write_all(prefix)?;
    server.input.write_all(&vec![b'x'; padding])?;
    server.input.write_all(ending)?;
    server.input.write_all(b"\n")?;
    server.input.flush()?;
    let mut line = String::new();
    server.output.read_line(&mut line)?;
    let response: Value = serde_json::from_str(&line)?;
    assert_eq!(response["id"], 1);
    drop(server.input);
    let output = server.child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn oversized_input_stops_without_protocol_output() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut server = fixture.server()?;
    server.input.write_all(&vec![b'x'; 1024 * 1024 + 1])?;
    server.input.write_all(b"\n")?;
    server.input.flush()?;
    drop(server.input);
    let output = server.child.wait_with_output()?;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "oversized input polluted stdout");
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds 1 MiB"));
    Ok(())
}

fn initialize_with_cli(fixture: &Fixture) -> Result<String, Box<dyn std::error::Error>> {
    let binary = env!("CARGO_BIN_EXE_pebble");
    for arguments in [
        vec!["--json", "init", fixture.repository.to_str().ok_or("path")?],
        vec![
            "--json",
            "register",
            fixture.repository.to_str().ok_or("path")?,
        ],
    ] {
        let output = Command::new(binary)
            .args(arguments)
            .env("HOME", &fixture.home)
            .env("USERPROFILE", &fixture.home)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    git(&fixture.repository, &["add", ".pebble/pebble.toml"])?;
    git(&fixture.repository, &["commit", "-qm", "configure pebble"])?;
    let output = Command::new(binary)
        .args([
            "--json",
            "index",
            fixture.repository.to_str().ok_or("path")?,
        ])
        .env("HOME", &fixture.home)
        .env("USERPROFILE", &fixture.home)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = fs::read_to_string(fixture.repository.join(".pebble/pebble.toml"))?;
    config
        .lines()
        .find_map(|line| line.strip_prefix("repository_id = \""))
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_owned)
        .ok_or_else(|| "missing repository id".into())
}

fn revision_string(value: &Value) -> Result<String, Box<dyn std::error::Error>> {
    let base = value["base_oid"].as_str().ok_or("base oid")?;
    Ok(value["dirty_digest"]
        .as_str()
        .map_or_else(|| base.to_owned(), |dirty| format!("{base}+dirty.{dirty}")))
}

fn git(repository: &Path, arguments: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(())
}
