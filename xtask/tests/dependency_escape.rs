#![forbid(unsafe_code)]

//! Dependency-governance escape-path integration tests.

#[path = "../src/dependency_records.rs"]
mod dependency_records;

use std::fs;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pebble-dependency-escape-{name}-{}",
        std::process::id()
    ))
}

fn write(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

#[test]
fn rejects_escaped_dependency_table_names() {
    let workspace = "[\"workspace\".\"dependenc\\u0069es\"]\nserde = \"=1.0.228\"\n";
    assert!(dependency_records::workspace_dependencies(workspace).is_err());

    let member = "[\"dependenc\\u0069es\"]\nserde = { workspace = true }\n";
    let violations = dependency_records::member_manifest_violations(
        member,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(!violations.is_empty());

    let dotted = "[workspace]\ndependenc\\u0069es.serde = { version = \"=1.0.228\" }\n";
    assert!(dependency_records::workspace_dependencies(dotted).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_under_manifest_search_roots() -> std::io::Result<()> {
    use std::os::unix::fs::symlink;

    let root = fixture("manifest-symlink");
    let external = fixture("manifest-symlink-external");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\n",
    )?;
    write(
        &root.join("config/dependency-decisions.txt"),
        "# crate exact-version scope decision record\n",
    )?;
    fs::create_dir_all(root.join("docs/dependencies"))?;
    fs::create_dir_all(root.join("crates/example"))?;
    write(
        &external.join("Cargo.toml"),
        "[package]\nname = \"external\"\nversion = \"1.0.0\"\n",
    )?;
    symlink(
        external.join("Cargo.toml"),
        root.join("crates/example/Cargo.toml"),
    )?;

    let result = dependency_records::check_repository(&root);
    assert!(result.is_err());

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(external)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_manifest_search_root() -> std::io::Result<()> {
    use std::os::unix::fs::symlink;

    let root = fixture("search-root-symlink");
    let external = fixture("search-root-symlink-external");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\n",
    )?;
    write(
        &root.join("config/dependency-decisions.txt"),
        "# crate exact-version scope decision record\n",
    )?;
    fs::create_dir_all(root.join("docs/dependencies"))?;
    write(
        &external.join("example/Cargo.toml"),
        "[package]\nname = \"external\"\nversion = \"1.0.0\"\n",
    )?;
    symlink(&external, root.join("crates"))?;

    assert!(dependency_records::check_repository(&root).is_err());

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(external)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_research_workspace_root() -> std::io::Result<()> {
    use std::os::unix::fs::symlink;

    let root = fixture("research-root-symlink");
    let external = fixture("research-root-symlink-external");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\n",
    )?;
    write(
        &root.join("config/dependency-decisions.txt"),
        "# crate exact-version scope decision record\n",
    )?;
    fs::create_dir_all(root.join("docs/dependencies"))?;
    write(
        &external.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\n",
    )?;
    symlink(&external, root.join("research"))?;

    assert!(dependency_records::check_repository(&root).is_err());

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(external)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_policy_files() -> std::io::Result<()> {
    use std::os::unix::fs::symlink;

    let root = fixture("policy-symlink");
    let external = fixture("policy-symlink-external");
    fs::create_dir_all(root.join("config"))?;
    fs::create_dir_all(root.join("docs/dependencies"))?;
    write(
        &external.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\n",
    )?;
    write(
        &external.join("decisions.txt"),
        "# crate exact-version scope decision record\n",
    )?;
    symlink(external.join("Cargo.toml"), root.join("Cargo.toml"))?;
    symlink(
        external.join("decisions.txt"),
        root.join("config/dependency-decisions.txt"),
    )?;

    assert!(dependency_records::check_repository(&root).is_err());
    fs::remove_file(root.join("config/dependency-decisions.txt"))?;
    write(
        &root.join("config/dependency-decisions.txt"),
        "# crate exact-version scope decision record\n",
    )?;
    assert!(dependency_records::check_repository(&root).is_err());

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(external)?;
    Ok(())
}
