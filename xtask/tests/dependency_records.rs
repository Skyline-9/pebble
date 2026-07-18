#![forbid(unsafe_code)]

//! Dependency-governance integration tests.

#[path = "../src/dependency_records.rs"]
mod dependency_records;
#[path = "support/plan2_fixture.rs"]
mod plan2_fixture;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const COMPLETE_RECORD: &str = "# Dependency decision\n\n## Requirement\n\n\
    ## Build in house\n\n## Candidates\n\n## Measurements\n\n## Decision\n";

fn fixture(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pebble-dependency-records-{name}-{}",
        std::process::id()
    ))
}

fn write(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

fn expected_string_error<T>(result: Result<T, String>) -> Result<String, String> {
    match result {
        Ok(_) => Err(String::from("expected an error")),
        Err(error) => Ok(error),
    }
}

fn expected_io_error<T>(result: Result<T, String>) -> std::io::Result<String> {
    match result {
        Ok(_) => Err(std::io::Error::other("expected an error")),
        Err(error) => Ok(error),
    }
}

fn production_fixture(name: &str) -> std::io::Result<PathBuf> {
    let root = fixture(name);
    fs::create_dir_all(&root)?;
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\n",
    )?;
    plan2_fixture::provision(&root)?;
    Ok(root)
}

#[test]
fn extracts_exact_workspace_dependencies() -> Result<(), String> {
    let manifest = r#"
[workspace.dependencies]
serde = "=1.0.228"
rusqlite = { version = "=0.40.1", features = ["bundled"] }

[workspace.lints.rust]
unsafe_code = "forbid"
"#;

    let dependencies = dependency_records::workspace_dependencies(manifest)?;
    assert_eq!(
        dependencies,
        BTreeMap::from([
            ("rusqlite".to_owned(), "0.40.1".to_owned()),
            ("serde".to_owned(), "1.0.228".to_owned()),
        ])
    );
    Ok(())
}

#[test]
fn rejects_non_exact_versions() -> Result<(), String> {
    let manifest = "[workspace.dependencies]\nserde = \"1.0\"\n";
    let error = expected_string_error(dependency_records::workspace_dependencies(manifest))?;
    assert!(error.contains("exact version"));
    Ok(())
}

#[test]
fn rejects_compound_exact_range() -> Result<(), String> {
    let manifest = "[workspace.dependencies]\nserde = \"=1.0.228, =1.0.229\"\n";
    let error = expected_string_error(dependency_records::workspace_dependencies(manifest))?;
    assert!(error.contains("exact version"));
    Ok(())
}

#[test]
fn rejects_multiline_workspace_dependency() {
    let manifest =
        "[workspace.dependencies]\nserde = {\nversion = \"=1.0.228\"\nfeatures = [\"derive\"]\n}\n";
    let result = dependency_records::workspace_dependencies(manifest);
    assert!(result.is_err());
}

#[test]
fn rejects_dotted_table_workspace_dependency() {
    let manifest =
        "[workspace.dependencies.serde]\nversion = \"=1.0.228\"\nfeatures = [\"derive\"]\n";
    let result = dependency_records::workspace_dependencies(manifest);
    assert!(result.is_err());
}

#[test]
fn rejects_quoted_workspace_dependency_table() {
    let manifest = "[\"workspace\".\"dependencies\"]\nserde = \"=1.0.228\"\n";
    assert!(dependency_records::workspace_dependencies(manifest).is_err());
}

#[test]
fn rejects_dotted_workspace_dependency_assignment() {
    let manifest = "[workspace]\ndependencies.serde = { version = \"=1.0.228\" }\n";
    assert!(dependency_records::workspace_dependencies(manifest).is_err());
}

#[test]
fn rejects_non_exact_inline_table_version() -> Result<(), String> {
    let manifest = "[workspace.dependencies]\nserde = { version = \"1.0\" }\n";
    let error = expected_string_error(dependency_records::workspace_dependencies(manifest))?;
    assert!(error.contains("exact version"));
    Ok(())
}

#[test]
fn rejects_workspace_git_source() -> Result<(), String> {
    let manifest =
        "[workspace.dependencies]\nexample = { version = \"=1.0.0\", git = \"https://x\" }\n";
    let error = expected_string_error(dependency_records::workspace_dependencies(manifest))?;
    assert!(error.contains("Git dependency"));
    Ok(())
}

#[test]
fn rejects_quoted_workspace_source_key() {
    let manifest = "[workspace.dependencies]\n\
        example = { version = \"=1.0.0\", \"git\" = \"https://x\" }\n";
    assert!(dependency_records::workspace_dependencies(manifest).is_err());
}

#[test]
fn rejects_workspace_path_source() -> Result<(), String> {
    let manifest =
        "[workspace.dependencies]\nexample = { version = \"=1.0.0\", path = \"../x\" }\n";
    let error = expected_string_error(dependency_records::workspace_dependencies(manifest))?;
    assert!(error.contains("source is forbidden"));
    Ok(())
}

#[test]
fn reports_missing_decision() -> Result<(), String> {
    let manifest = "[workspace.dependencies]\nserde = \"=1.0.228\"\n";
    let decisions = "# crate exact-version scope decision record\n";

    let missing = dependency_records::missing_decisions(manifest, decisions)?;
    assert_eq!(missing, vec!["serde"]);
    Ok(())
}

#[test]
fn rejects_incomplete_markdown_record() {
    let incomplete = "# Dependency decision: `serde`\n\n## Requirement\n";
    let missing = dependency_records::missing_record_sections(incomplete);
    assert!(missing.contains(&"## Build in house"));
    assert!(missing.contains(&"## Candidates"));
    assert!(missing.contains(&"## Measurements"));
    assert!(missing.contains(&"## Decision"));
}

#[test]
fn rejects_member_owned_third_party_version() {
    let manifest = "[dependencies]\nserde = \"=1.0.228\"\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(
        violations
            .iter()
            .any(|item| item.contains("workspace = true"))
    );
}

#[test]
fn rejects_multiline_member_dependency() {
    let manifest = "[dependencies]\nserde = {\nversion = \"=1.0.228\"\nworkspace = true\n}\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(
        violations
            .iter()
            .any(|item| item.contains("multiline dependency entry"))
    );
}

#[test]
fn rejects_target_specific_member_owned_version() {
    let manifest = "[target.'cfg(unix)'.dependencies]\nserde = \"=1.0.228\"\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(
        violations
            .iter()
            .any(|item| item.contains("workspace = true"))
    );
}

#[test]
fn rejects_dotted_member_dependency_key() {
    let manifest = "[dependencies]\nserde.workspace = true\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(!violations.is_empty());
}

#[test]
fn rejects_dotted_table_member_dependency() {
    let manifest = "[target.'cfg(unix)'.dependencies.serde]\nversion = \"=1.0.228\"\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(
        violations
            .iter()
            .any(|item| item.contains("multiline dependency entry"))
    );
}

#[test]
fn rejects_quoted_member_dependency_table() {
    let manifest = "[\"dependencies\"]\nserde = { workspace = true }\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(!violations.is_empty());
}

#[test]
fn rejects_member_git_dependency() {
    let manifest = "[dependencies]\nexample = { git = \"https://example.com/x\" }\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(
        violations
            .iter()
            .any(|item| item.contains("Git dependency"))
    );
}

#[test]
fn rejects_quoted_member_source_key() {
    let manifest = "[dependencies]\nexample = { \"git\" = \"https://example.com/x\" }\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(!violations.is_empty());
}

#[test]
fn rejects_member_registry_source() {
    let manifest = "[dependencies]\nserde = { workspace = true, registry = \"other-registry\" }\n";
    let violations = dependency_records::member_manifest_violations(
        manifest,
        Path::new("crates/example"),
        Path::new("."),
    );
    assert!(
        violations
            .iter()
            .any(|item| item.contains("registry source"))
    );
}

#[test]
fn rejects_external_path_dependency() -> std::io::Result<()> {
    let root = fixture("external-path");
    let member = root.join("crates/example");
    let external = fixture("external-package");
    fs::create_dir_all(&member)?;
    fs::create_dir_all(&external)?;
    let manifest = format!(
        "[dependencies]\nexample = {{ path = \"{}\" }}\n",
        external.display()
    );

    let violations = dependency_records::member_manifest_violations(&manifest, &member, &root);
    assert!(
        violations
            .iter()
            .any(|item| item.contains("external path dependency"))
    );

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(external)?;
    Ok(())
}

#[test]
fn rejects_missing_internal_path_dependency() -> std::io::Result<()> {
    let root = fixture("missing-path");
    let member = root.join("crates/example");
    fs::create_dir_all(&member)?;
    let manifest = "[dependencies]\nexample = { path = \"../missing\" }\n";

    let violations = dependency_records::member_manifest_violations(manifest, &member, &root);
    assert!(
        violations
            .iter()
            .any(|item| item.contains("missing path dependency"))
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn permits_canonical_internal_path_dependency() -> std::io::Result<()> {
    let root = fixture("internal-path");
    let member = root.join("crates/example");
    fs::create_dir_all(&member)?;
    fs::create_dir_all(root.join("crates/internal"))?;
    let manifest = "[dependencies]\ninternal = { path = \"../internal\" }\n";

    let violations = dependency_records::member_manifest_violations(manifest, &member, &root);
    assert!(violations.is_empty(), "{violations:?}");

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn reports_workspace_version_mismatch() -> std::io::Result<()> {
    let root = production_fixture("version-mismatch")?;
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nserde = \"=1.0.228\"\n",
    )?;
    write(
        &root.join("config/dependency-decisions.txt"),
        "serde 1.0.227 production adopt docs/dependencies/serde.md\n",
    )?;
    write(&root.join("docs/dependencies/serde.md"), COMPLETE_RECORD)?;

    let error = expected_io_error(dependency_records::check_repository(&root))?;
    assert!(error.contains("differs from decision"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rejects_research_member_owned_version() -> std::io::Result<()> {
    let root = production_fixture("research-version")?;
    write(
        &root.join("research/Cargo.toml"),
        "[workspace]\nmembers = [\"spikes/example\"]\n\n\
         [workspace.dependencies]\nserde = \"=1.0.228\"\n",
    )?;
    write(
        &root.join("research/spikes/example/Cargo.toml"),
        "[package]\nname = \"example\"\nversion = \"1.0.0\"\n\n\
         [dependencies]\nserde = \"=1.0.228\"\n",
    )?;
    write(
        &root.join("config/dependency-decisions.txt"),
        "serde 1.0.228 research defer docs/dependencies/serde.md\n",
    )?;
    write(&root.join("docs/dependencies/serde.md"), COMPLETE_RECORD)?;

    let error = expected_io_error(dependency_records::check_repository(&root))?;
    assert!(error.contains("workspace = true"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn permits_rejected_research_workspace_entry() -> std::io::Result<()> {
    let root = production_fixture("research-reject")?;
    write(
        &root.join("research/Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nother = \"=1.0.0\"\n",
    )?;
    write(
        &root.join("config/dependency-decisions.txt"),
        &format!(
            "{}other 1.0.0 research reject docs/dependencies/record.md\n",
            plan2_fixture::approved_decisions()
        ),
    )?;
    let result = dependency_records::check_repository(&root);
    assert!(result.is_ok(), "{result:?}");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn permits_unused_rejected_production_decision() -> std::io::Result<()> {
    let root = production_fixture("unused-production-reject")?;
    write(
        &root.join("config/dependency-decisions.txt"),
        &format!(
            "{}other 1.0.0 production reject docs/dependencies/record.md\n",
            plan2_fixture::approved_decisions()
        ),
    )?;
    let result = dependency_records::check_repository(&root);
    assert!(result.is_ok(), "{result:?}");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rejects_missing_record_path() -> std::io::Result<()> {
    let root = production_fixture("missing-record")?;
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nserde = \"=1.0.228\"\n",
    )?;
    write(
        &root.join("config/dependency-decisions.txt"),
        "serde 1.0.228 production adopt docs/dependencies/serde.md\n",
    )?;

    let error = expected_io_error(dependency_records::check_repository(&root))?;
    assert!(error.contains("record path"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_record_directory_outside_repository() -> std::io::Result<()> {
    use std::os::unix::fs::symlink;

    let root = production_fixture("record-symlink")?;
    let external = fixture("record-symlink-external");
    fs::create_dir_all(&external)?;
    write(&external.join("serde.md"), "# serde\n")?;
    fs::remove_dir_all(root.join("docs/dependencies"))?;
    fs::create_dir_all(root.join("docs"))?;
    symlink(&external, root.join("docs/dependencies"))?;
    write(
        &root.join("config/dependency-decisions.txt"),
        "serde 1.0.228 production reject docs/dependencies/serde.md\n",
    )?;

    let error = expected_io_error(dependency_records::check_repository(&root))?;
    assert!(error.contains("outside repository"));

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(external)?;
    Ok(())
}

#[test]
fn plan_requirement_accepts_one_audited_alternative() -> Result<(), String> {
    let decisions = "gix 0.83.0 research reject x\nsystem-git n/a boundary adopt x\n";
    let requirements = "git research:gix@0.83.0|boundary:system-git@n/a\n";
    assert!(dependency_records::invalid_plan_requirements(decisions, requirements)?.is_empty());
    Ok(())
}

#[test]
fn promoted_plan2_candidate_requires_frozen_version() {
    use dependency_records::invalid_plan_requirements as check;
    let requirements = "serialization research:serde@1.0.228\n";
    assert_eq!(
        check("serde 1.0.228 production adopt x\n", requirements),
        Ok(Vec::new())
    );
    assert_eq!(
        check("serde 1.0.229 production adopt x\n", requirements),
        Ok(vec![String::from("serialization")])
    );
}
