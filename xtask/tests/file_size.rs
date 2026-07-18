#![forbid(unsafe_code)]

//! File-size gate integration tests.

#[path = "../src/file_size.rs"]
mod file_size;

use std::fs;

fn fixture(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("pebble-file-size-{name}-{}", std::process::id()))
}

#[test]
fn rejects_production_file_above_300_lines() -> std::io::Result<()> {
    let root = fixture("production");
    let source = root.join("crates/example/src/lib.rs");
    fs::create_dir_all(root.join("crates/example/src"))?;
    fs::write(&source, "line\n".repeat(301))?;

    let violations = file_size::check(&root)?;
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].limit, 300);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn permits_test_file_up_to_500_lines() -> std::io::Result<()> {
    let root = fixture("test");
    let source = root.join("crates/example/tests/integration.rs");
    fs::create_dir_all(root.join("crates/example/tests"))?;
    fs::write(&source, "line\n".repeat(500))?;

    let violations = file_size::check(&root)?;
    assert!(violations.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}
