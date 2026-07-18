#![forbid(unsafe_code)]

//! CLI rejection tests for unsafe local state reads.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "pebble-cli-state-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        fs::create_dir_all(home.join(".pebble/v1"))?;
        Ok(Self { root, home })
    }

    fn state(&self) -> PathBuf {
        self.home.join(".pebble/v1")
    }

    fn run(&self, arguments: &[&str]) -> std::io::Result<Output> {
        Command::new(env!("CARGO_BIN_EXE_pebble"))
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .args(arguments)
            .output()
    }

    fn write_registration(&self) -> std::io::Result<()> {
        fs::write(
            self.state().join("registry.json"),
            format!(
                concat!(
                    "{{\"schema\":1,\"registrations\":[{{",
                    "\"repository_id\":\"local\",",
                    "\"checkout\":\"{}\",",
                    "\"alternate_worktree\":false",
                    "}}]}}"
                ),
                self.root.display()
            ),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(unix)]
#[test]
fn registry_symlink_is_an_operational_error() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("registry-symlink")?;
    let target = fixture.state().join("target.json");
    fs::write(&target, b"{\"schema\":1,\"registrations\":[]}")?;
    symlink(target, fixture.state().join("registry.json"))?;

    let output = fixture.run(&["--json", "health", "--repository", "local"])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn oversized_registry_is_an_operational_error() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("registry-oversize")?;
    fs::write(
        fixture.state().join("registry.json"),
        vec![b' '; 1024 * 1024 + 1],
    )?;

    let output = fixture.run(&["--json", "health", "--repository", "local"])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("1 MiB"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn special_trace_file_is_an_operational_error() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("trace-special")?;
    fixture.write_registration()?;
    let repository = fixture.state().join("repos/local");
    fs::create_dir_all(&repository)?;
    symlink("/dev/zero", repository.join("traces.jsonl"))?;

    let output = fixture.run(&["--json", "traces", "--repository", "local"])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    Ok(())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
