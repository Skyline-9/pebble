#![forbid(unsafe_code)]

//! Production quality-gate integration tests.

use std::process::Command;

#[test]
fn version_prints_to_stdout() -> Result<(), Box<dyn std::error::Error>> {
    let binary = env!("CARGO_BIN_EXE_pebble");
    let output = Command::new(binary).arg("--version").output()?;

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "pebble 1.0.0\n");
    assert!(output.stderr.is_empty());
    Ok(())
}
