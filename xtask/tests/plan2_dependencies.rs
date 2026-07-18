#![forbid(unsafe_code)]

//! Plan 2 capability-contract integration tests.

#[path = "../src/dependency_records.rs"]
mod dependency_records;

use std::fs;
use std::path::{Path, PathBuf};

const COMPLETE_INVENTORY: &str = "\
serialization research:serde@1.0.228
json research:serde_json@1.0.150
errors research:thiserror@2.0.18
cli research:clap@4.6.1
runtime research:tokio@1.52.3
mcp research:rmcp@1.8.0
graph research:rusqlite@0.40.1
lexical research:tantivy@0.26.1
parser research:tree-sitter@0.26.8
grammar-c research:tree-sitter-c@0.24.2
grammar-c-sharp research:tree-sitter-c-sharp@0.23.5
grammar-cpp research:tree-sitter-cpp@0.23.4
grammar-go research:tree-sitter-go@0.25.0
grammar-java research:tree-sitter-java@0.23.5
grammar-javascript research:tree-sitter-javascript@0.25.0
grammar-kotlin research:tree-sitter-kotlin-ng@1.1.0
grammar-python research:tree-sitter-python@0.25.0
grammar-ruby research:tree-sitter-ruby@0.23.1
grammar-rust research:tree-sitter-rust@0.24.2
grammar-swift research:tree-sitter-swift@0.7.3
grammar-typescript research:tree-sitter-typescript@0.23.2
symbols research:scip@0.6.1
toml research:toml@1.1.2
markdown research:pulldown-cmark@0.13.4
yaml research:yaml-rust2@0.11.0
git research:gix@0.83.0|boundary:system-git@n/a
traversal research:ignore@0.4.28
hash research:blake3@1.8.5
ids research:ulid@1.2.1
watch research:notify@8.2.0
";

fn error(result: Result<(), String>) -> Result<String, String> {
    match result {
        Ok(()) => Err(String::from("expected an error")),
        Err(error) => Ok(error),
    }
}

fn fixture(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pebble-plan2-dependencies-{name}-{}",
        std::process::id()
    ))
}

fn write(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

fn adopted_decisions() -> String {
    COMPLETE_INVENTORY
        .lines()
        .filter_map(|line| line.split_once(' '))
        .flat_map(|(_capability, candidates)| {
            candidates.split('|').filter_map(|candidate| {
                let (scope, coordinate) = candidate.split_once(':')?;
                let (name, version) = coordinate.rsplit_once('@')?;
                let decision = if name == "gix" { "reject" } else { "adopt" };
                Some(format!(
                    "{name} {version} {scope} {decision} docs/dependencies/record.md\n"
                ))
            })
        })
        .collect()
}

fn accepted_evidence() -> String {
    String::from(
        r#"{
  "schema": 1,
  "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "acceptance": "accepted",
  "required_macos_result": {
    "target": "aarch64-apple-darwin",
    "result_spikes": ["foundation", "mcp-runtime", "storage-search", "ingestion", "documents-git-watch"],
    "result_id": "local",
    "status": "passed"
  },
  "plan2_contract": {"status": "accepted"}
}"#,
    )
}

fn result_with_targets(spike: &str, targets: &str) -> String {
    format!(
        r#"{{
  "schema": 1,
  "spike": "{spike}",
  "fixture_hash": "blake3:test",
  "dependencies": [],
  "targets": [{targets}]
}}"#
    )
}

fn accepted_result(spike: &str) -> String {
    let failures = if spike == "documents-git-watch" {
        r#"["gix_0_83_0_rejected_malformed_index_panics_in_isolated_process"]"#
    } else {
        "[]"
    };
    result_with_targets(
        spike,
        &format!(
            r#"{{"target": "aarch64-apple-darwin", "ci_run_id": "local", "failures": {failures}}}"#
        ),
    )
}

fn accepted_repository(name: &str) -> std::io::Result<PathBuf> {
    let root = fixture(name);
    fs::create_dir_all(&root)?;
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\n",
    )?;
    write(
        &root.join("config/dependency-decisions.txt"),
        &adopted_decisions(),
    )?;
    write(
        &root.join("docs/dependencies/record.md"),
        "# Dependency decision\n\n## Requirement\n\n## Build in house\n\n## Candidates\n\n## Measurements\n\n## Decision\n",
    )?;
    write(
        &root.join("config/plan2-dependencies.txt"),
        COMPLETE_INVENTORY,
    )?;
    write(
        &root.join("research/results/foundation-acceptance.json"),
        &accepted_evidence(),
    )?;
    for spike in [
        "foundation",
        "mcp-runtime",
        "storage-search",
        "ingestion",
        "documents-git-watch",
    ] {
        write(
            &root.join(format!("research/results/{spike}.json")),
            &accepted_result(spike),
        )?;
    }
    Ok(root)
}

#[test]
fn adopted_plan2_accepts_checked_in_local_macos_evidence() -> std::io::Result<()> {
    let root = accepted_repository("accepted-evidence")?;
    let result = dependency_records::check_repository(&root);
    assert!(result.is_ok(), "{result:?}");
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn adopted_plan2_rejects_missing_mismatched_or_non_macos_evidence() -> std::io::Result<()> {
    let root = accepted_repository("missing-evidence")?;
    fs::remove_file(root.join("research/results/foundation.json"))?;
    let error = dependency_records::check_repository(&root)
        .err()
        .ok_or_else(|| std::io::Error::other("expected an error"))?;
    assert!(error.contains("read"));
    fs::remove_dir_all(&root)?;

    let root = accepted_repository("mismatched-evidence")?;
    write(
        &root.join("research/results/foundation.json"),
        &result_with_targets(
            "foundation",
            r#"{"target": "aarch64-apple-darwin", "ci_run_id": "other", "failures": []}"#,
        ),
    )?;
    let error = dependency_records::check_repository(&root)
        .err()
        .ok_or_else(|| std::io::Error::other("expected an error"))?;
    assert!(error.contains("does not match local macOS result ID"));
    fs::remove_dir_all(&root)?;

    let root = accepted_repository("linux-evidence")?;
    write(
        &root.join("research/results/foundation.json"),
        &result_with_targets(
            "foundation",
            r#"{"target": "x86_64-unknown-linux-gnu", "ci_run_id": "local", "failures": []}"#,
        ),
    )?;
    let error = dependency_records::check_repository(&root)
        .err()
        .ok_or_else(|| std::io::Error::other("expected an error"))?;
    assert!(error.contains("must contain a macOS result"));
    fs::remove_dir_all(&root)?;

    let root = accepted_repository("ubuntu-evidence")?;
    write(
        &root.join("research/results/foundation.json"),
        &result_with_targets(
            "foundation",
            r#"{"target": "aarch64-apple-darwin", "ci_run_id": "local", "failures": []}, {"target": "x86_64-unknown-linux-gnu", "ci_run_id": "local", "failures": []}"#,
        ),
    )?;
    let error = dependency_records::check_repository(&root)
        .err()
        .ok_or_else(|| std::io::Error::other("expected an error"))?;
    assert!(error.contains("must contain exactly one macOS result"));
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn adopted_plan2_requires_the_gix_rejection_evidence() -> std::io::Result<()> {
    let root = accepted_repository("missing-gix-rejection")?;
    write(
        &root.join("research/results/documents-git-watch.json"),
        &result_with_targets(
            "documents-git-watch",
            r#"{"target": "aarch64-apple-darwin", "ci_run_id": "local", "failures": []}"#,
        ),
    )?;
    let error = dependency_records::check_repository(&root)
        .err()
        .ok_or_else(|| std::io::Error::other("expected an error"))?;
    assert!(error.contains("documents-git-watch evidence must preserve the gix rejection"));
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn repository_gate_rejects_empty_or_comment_only_inventory() -> std::io::Result<()> {
    for (name, inventory) in [("empty-inventory", ""), ("comment-inventory", "# Plan 2\n")] {
        let root = accepted_repository(name)?;
        write(&root.join("config/plan2-dependencies.txt"), inventory)?;
        let error = dependency_records::check_repository(&root)
            .err()
            .ok_or_else(|| std::io::Error::other("expected an error"))?;
        assert!(error.contains("capability inventory cannot be empty"));
        fs::remove_dir_all(root)?;
    }
    Ok(())
}

#[test]
fn plan2_inventory_rejects_empty_missing_unknown_duplicate_and_malformed_entries()
-> Result<(), String> {
    let empty = error(dependency_records::validate_plan2_inventory(""))?;
    assert!(empty.contains("empty"));

    let comment_only = error(dependency_records::validate_plan2_inventory("# contract\n"))?;
    assert!(comment_only.contains("empty"));

    let missing = error(dependency_records::validate_plan2_inventory(
        &COMPLETE_INVENTORY.replacen("watch research:notify@8.2.0\n", "", 1),
    ))?;
    assert!(missing.contains("missing Plan 2 capabilities: watch"));

    let unknown = error(dependency_records::validate_plan2_inventory(&format!(
        "{COMPLETE_INVENTORY}unknown research:unknown@1.0.0\n"
    )))?;
    assert!(unknown.contains("unknown Plan 2 capability: unknown"));

    let duplicate = error(dependency_records::validate_plan2_inventory(&format!(
        "{COMPLETE_INVENTORY}watch research:notify@8.2.0\n"
    )))?;
    assert!(duplicate.contains("duplicate Plan 2 capability: watch"));

    let malformed = error(dependency_records::validate_plan2_inventory(
        &COMPLETE_INVENTORY.replacen(
            "serialization research:serde@1.0.228",
            "serialization research:serde@1.0.228 trailing",
            1,
        ),
    ))?;
    assert!(malformed.contains("invalid Plan 2 requirement"));
    Ok(())
}

#[test]
fn plan2_inventory_requires_the_exact_capability_candidates() -> Result<(), String> {
    dependency_records::validate_plan2_inventory(COMPLETE_INVENTORY)?;

    let altered = error(dependency_records::validate_plan2_inventory(
        &COMPLETE_INVENTORY.replacen(
            "research:gix@0.83.0|boundary:system-git@n/a",
            "boundary:system-git@n/a",
            1,
        ),
    ))?;
    assert!(altered.contains("incorrect candidates for git"));
    Ok(())
}

#[test]
fn plan_requirement_rejects_deferred_candidate() -> Result<(), String> {
    let decisions = "gix 0.83.0 research defer docs/dependencies/git.md\n";
    let requirements = "git research:gix@0.83.0|boundary:system-git@n/a\n";

    assert_eq!(
        dependency_records::invalid_plan_requirements(decisions, requirements)?,
        vec!["git"]
    );
    Ok(())
}

#[test]
fn plan_requirement_rejects_wrong_version_scope_unknown_and_two_choices() -> Result<(), String> {
    let requirements = "git research:gix@0.83.0|boundary:system-git@n/a\n";
    let wrong_version = "gix 0.82.0 research adopt docs/dependencies/git.md\n";
    assert_eq!(
        dependency_records::invalid_plan_requirements(wrong_version, requirements)?,
        vec!["git"]
    );

    let wrong_scope = "gix 0.83.0 development adopt docs/dependencies/git.md\n";
    assert_eq!(
        dependency_records::invalid_plan_requirements(wrong_scope, requirements)?,
        vec!["git"]
    );

    let unknown = "other 1.0.0 research adopt docs/dependencies/git.md\n";
    assert_eq!(
        dependency_records::invalid_plan_requirements(unknown, requirements)?,
        vec!["git"]
    );

    let ambiguous = "\
gix 0.83.0 research adopt docs/dependencies/git.md\n\
system-git n/a boundary adopt docs/dependencies/git.md\n";
    assert_eq!(
        dependency_records::invalid_plan_requirements(ambiguous, requirements)?,
        vec!["git"]
    );
    Ok(())
}

#[test]
fn plan_requirement_rejects_unapproved_scope() -> Result<(), String> {
    let decisions = "gix 0.83.0 research adopt docs/dependencies/git.md\n";
    let requirements = "git production:gix@0.83.0\n";
    let result = dependency_records::invalid_plan_requirements(decisions, requirements);
    let error = result
        .err()
        .ok_or_else(|| String::from("expected an error"))?;

    assert!(error.contains("unapproved Plan 2 scope"));
    Ok(())
}
