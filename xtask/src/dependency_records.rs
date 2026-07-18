#![allow(dead_code, reason = "shared by the xtask binary and integration tests")]

#[path = "dependency_records/capability.rs"]
mod capability;
#[path = "dependency_records/decisions.rs"]
mod decisions;
#[path = "dependency_records/foundation.rs"]
mod foundation;
#[path = "dependency_records/manifest.rs"]
mod manifest;
#[path = "dependency_records/path_policy.rs"]
mod path_policy;
#[path = "dependency_records/repository.rs"]
mod repository;
#[path = "dependency_records/syntax.rs"]
mod syntax;

use decisions::{DecisionRecord, parse_decisions};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const REQUIRED_RECORD_SECTIONS: [&str; 5] = [
    "## Requirement",
    "## Build in house",
    "## Candidates",
    "## Measurements",
    "## Decision",
];

/// Extract exact-version dependency names from `[workspace.dependencies]`.
///
/// # Errors
///
/// Returns an error for malformed entries, forbidden sources, or non-exact
/// versions.
pub fn workspace_dependencies(manifest: &str) -> Result<BTreeMap<String, String>, String> {
    manifest::workspace_dependencies(manifest)
}

/// Return workspace dependencies with no decision registry entry.
///
/// # Errors
///
/// Returns an error when either input violates its line-oriented schema.
pub fn missing_decisions(manifest: &str, decisions: &str) -> Result<Vec<String>, String> {
    let dependencies = workspace_dependencies(manifest)?;
    let records = parse_decisions(decisions)?;
    Ok(dependencies
        .into_keys()
        .filter(|name| !records.contains_key(name))
        .collect())
}

/// Return required dependency-record headings absent from Markdown.
#[must_use]
pub fn missing_record_sections(markdown: &str) -> Vec<&'static str> {
    REQUIRED_RECORD_SECTIONS
        .into_iter()
        .filter(|heading| !markdown.lines().any(|line| line == *heading))
        .collect()
}

/// Return policy violations from one workspace member manifest.
#[must_use]
pub fn member_manifest_violations(
    manifest: &str,
    member_dir: &Path,
    repository_root: &Path,
) -> Vec<String> {
    manifest::member_manifest_violations(manifest, member_dir, repository_root)
}

/// Return Plan 2 capabilities without exactly one approved alternative.
///
/// # Errors
///
/// Returns an error when the decision registry or capability contract is
/// malformed.
pub fn invalid_plan_requirements(
    decisions: &str,
    requirements: &str,
) -> Result<Vec<String>, String> {
    let records = parse_decisions(decisions)?;
    capability::invalid_requirements(&records, requirements)
}

/// Validate that Plan 2 declares its complete, exact capability inventory.
///
/// # Errors
///
/// Returns an error when an inventory entry is empty, missing, unknown,
/// duplicate, malformed, or altered.
pub fn validate_plan2_inventory(requirements: &str) -> Result<(), String> {
    capability::validate_inventory(requirements)
}

/// Validate dependency-decision coverage for a repository root.
///
/// # Errors
///
/// Returns an error when required files are unreadable or a dependency policy
/// is violated.
pub fn check_repository(root: &Path) -> Result<(), String> {
    let repository_root = fs::canonicalize(root)
        .map_err(|error| format!("canonicalize repository root {}: {error}", root.display()))?;
    let decisions_path = repository_root.join("config/dependency-decisions.txt");
    let decisions_text = read_internal_file(&repository_root, &decisions_path)?;
    let records = parse_decisions(&decisions_text)?;
    let requirements_path = repository_root.join("config/plan2-dependencies.txt");
    let requirements = read_internal_file(&repository_root, &requirements_path)?;
    capability::validate_inventory(&requirements)?;

    validate_records(&repository_root, &records)?;
    check_workspace(&repository_root, WorkspaceKind::Production, &records)?;
    check_member_manifests(
        &repository_root,
        &[
            repository_root.join("crates"),
            repository_root.join("xtask"),
        ],
    )?;

    let research_root = repository_root.join("research");
    if fs::symlink_metadata(&research_root).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(format!(
            "research workspace root cannot be a symlink: {}",
            research_root.display()
        ));
    }
    if research_root.join("Cargo.toml").is_file() {
        check_workspace(&research_root, WorkspaceKind::Research, &records)?;
        check_member_manifests(
            &repository_root,
            &[
                research_root.join("spike-support"),
                research_root.join("spikes"),
            ],
        )?;
    }
    validate_plan2_capabilities(&repository_root, &records, &requirements)?;
    Ok(())
}

fn validate_plan2_capabilities(
    repository_root: &Path,
    records: &BTreeMap<String, DecisionRecord>,
    requirements: &str,
) -> Result<(), String> {
    let invalid = capability::invalid_requirements(records, requirements)?;
    foundation::validate(repository_root, invalid.is_empty())?;
    if invalid.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Plan 2 capabilities lack exactly one approved alternative: {}",
            invalid.join(", ")
        ))
    }
}

#[derive(Clone, Copy)]
enum WorkspaceKind {
    Production,
    Research,
}

fn check_workspace(
    workspace_root: &Path,
    kind: WorkspaceKind,
    records: &BTreeMap<String, DecisionRecord>,
) -> Result<(), String> {
    let manifest_path = workspace_root.join("Cargo.toml");
    let canonical_root = fs::canonicalize(workspace_root)
        .map_err(|error| format!("canonicalize {}: {error}", workspace_root.display()))?;
    let manifest = read_internal_file(&canonical_root, &manifest_path)?;
    let dependencies = workspace_dependencies(&manifest)?;

    for (name, version) in dependencies {
        let record = records
            .get(&name)
            .ok_or_else(|| format!("{name} has no dependency decision"))?;
        if record.version != version {
            return Err(format!(
                "{name} manifest version {version} differs from decision {}",
                record.version
            ));
        }
        if matches!(kind, WorkspaceKind::Production) {
            if record.decision != "adopt" {
                return Err(format!("{name} is in production but is not adopted"));
            }
            if matches!(record.scope.as_str(), "research" | "boundary") {
                return Err(format!(
                    "{name} has invalid production scope {}",
                    record.scope
                ));
            }
        }
    }
    Ok(())
}

fn read_internal_file(root: &Path, path: &Path) -> Result<String, String> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!(
            "policy file cannot be a symlink: {}",
            path.display()
        ));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("canonicalize {}: {error}", path.display()))?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(format!("policy file escapes root: {}", path.display()));
    }
    fs::read_to_string(&canonical).map_err(|error| format!("read {}: {error}", canonical.display()))
}

fn validate_records(
    repository_root: &Path,
    records: &BTreeMap<String, DecisionRecord>,
) -> Result<(), String> {
    for (name, record) in records {
        validate_record_path(repository_root, name, &record.record_path)?;
    }
    Ok(())
}

fn validate_record_path(root: &Path, name: &str, record_path: &str) -> Result<(), String> {
    let relative = Path::new(record_path);
    let expected_directory = Path::new("docs/dependencies");
    if relative.is_absolute()
        || !relative.starts_with(expected_directory)
        || relative
            .extension()
            .is_none_or(|extension| extension != "md")
    {
        return Err(format!(
            "{name} record path must be a Markdown file under docs/dependencies: {record_path}"
        ));
    }

    let canonical_directory = fs::canonicalize(root.join(expected_directory)).map_err(|error| {
        format!("canonicalize dependency record path directory for {name}: {error}")
    })?;
    if !canonical_directory.starts_with(root) {
        return Err(format!(
            "dependency record directory resolves outside repository: {}",
            canonical_directory.display()
        ));
    }
    let canonical_record = fs::canonicalize(root.join(relative))
        .map_err(|error| format!("record path {record_path} for {name}: {error}"))?;
    if !canonical_record.starts_with(root)
        || !canonical_record.starts_with(&canonical_directory)
        || !canonical_record.is_file()
    {
        return Err(format!(
            "{name} record path is not an internal Markdown file: {record_path}"
        ));
    }
    let markdown = fs::read_to_string(&canonical_record)
        .map_err(|error| format!("read {record_path} for {name}: {error}"))?;
    let missing_sections = missing_record_sections(&markdown);
    if !missing_sections.is_empty() {
        return Err(format!(
            "{record_path} is missing: {}",
            missing_sections.join(", ")
        ));
    }
    Ok(())
}

fn check_member_manifests(repository_root: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let manifests = repository::manifest_paths(roots)?;
    let mut violations = Vec::new();
    for manifest_path in manifests {
        let manifest = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
        let member_dir = manifest_path
            .parent()
            .ok_or_else(|| format!("manifest has no parent: {}", manifest_path.display()))?;
        let display_path = manifest_path
            .strip_prefix(repository_root)
            .unwrap_or(&manifest_path);
        violations.extend(
            member_manifest_violations(&manifest, member_dir, repository_root)
                .into_iter()
                .map(|violation| format!("{}: {violation}", display_path.display())),
        );
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("\n"))
    }
}
