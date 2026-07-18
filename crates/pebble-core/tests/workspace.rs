//! Workspace manifest durability and federated search tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::domain::RepositoryId;
use pebble_core::workspace::{
    RankedHit, RepositoryResultSource, WorkspaceError, WorkspaceManifest, federated_search,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "pebble-workspace-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repository(name: &str) -> Result<RepositoryId, pebble_core::error::DomainError> {
    RepositoryId::try_from(name.to_owned())
}

#[test]
fn create_add_remove_list_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("round-trip")?;
    let alpha = repository("repo.alpha")?;
    let beta = repository("repo.beta")?;

    let mut manifest = WorkspaceManifest::create(state.path(), "team")?;
    assert_eq!(manifest.name(), "team");
    assert!(manifest.repositories().is_empty());

    manifest.add_repository(alpha.clone())?;
    manifest.add_repository(beta.clone())?;
    manifest.add_repository(alpha.clone())?;
    assert_eq!(manifest.repositories(), &[alpha.clone(), beta.clone()]);

    let loaded = WorkspaceManifest::load(state.path(), "team")?;
    assert_eq!(loaded.repositories(), &[alpha.clone(), beta.clone()]);

    let mut loaded = loaded;
    loaded.remove_repository(&alpha)?;
    loaded.remove_repository(&repository("repo.absent")?)?;
    assert_eq!(loaded.repositories(), std::slice::from_ref(&beta));

    let reloaded = WorkspaceManifest::load(state.path(), "team")?;
    assert_eq!(reloaded.repositories(), &[beta]);

    assert!(WorkspaceManifest::create(state.path(), "personal-only").is_ok());
    assert_eq!(
        WorkspaceManifest::list(state.path())?,
        vec!["personal-only".to_owned(), "team".to_owned()]
    );
    Ok(())
}

#[test]
fn list_on_missing_state_root_returns_empty() -> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("missing-list")?;
    assert!(WorkspaceManifest::list(&state.path().join("nested"))?.is_empty());
    Ok(())
}

#[test]
fn load_on_missing_workspace_reports_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("missing-load")?;
    let result = WorkspaceManifest::load(state.path(), "absent");
    assert!(matches!(result, Err(WorkspaceError::NotFound(ref name)) if name == "absent"));
    Ok(())
}

#[test]
fn create_rejects_a_duplicate_workspace_name() -> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("duplicate")?;
    WorkspaceManifest::create(state.path(), "team")?;
    let result = WorkspaceManifest::create(state.path(), "team");
    assert!(matches!(result, Err(WorkspaceError::AlreadyExists(ref name)) if name == "team"));
    Ok(())
}

#[test]
fn manifest_writes_survive_a_simulated_crash_leaving_the_temp_file_behind()
-> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("crash")?;
    let alpha = repository("repo.alpha")?;
    let mut manifest = WorkspaceManifest::create(state.path(), "team")?;
    manifest.add_repository(alpha.clone())?;

    let workspaces_dir = state.path().join("workspaces");
    let stray = workspaces_dir.join(format!(".team-{}-999999.tmp", std::process::id()));
    fs::write(&stray, b"leftover from a crashed writer")?;

    let reloaded = WorkspaceManifest::load(state.path(), "team")?;
    assert_eq!(reloaded.repositories(), &[alpha]);
    assert!(workspaces_dir.join("team.json").is_file());
    assert_eq!(
        fs::read_to_string(&stray)?,
        "leftover from a crashed writer"
    );

    let mut reloaded = reloaded;
    reloaded.add_repository(repository("repo.beta")?)?;
    assert_eq!(
        fs::read_to_string(&stray)?,
        "leftover from a crashed writer"
    );
    assert!(stray.is_file());
    Ok(())
}

#[test]
fn name_validation_rejects_unsafe_identifiers() -> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("names")?;
    let long_name = "a".repeat(129);
    for name in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "../escape",
        "team name",
        "team!",
        long_name.as_str(),
    ] {
        assert!(
            WorkspaceManifest::create(state.path(), name).is_err(),
            "expected {name:?} to be rejected"
        );
    }

    let max_name = "a".repeat(128);
    assert!(WorkspaceManifest::create(state.path(), &max_name).is_ok());
    Ok(())
}

struct FakeSource {
    hits: Vec<(f64, &'static str)>,
}

impl RepositoryResultSource for FakeSource {
    type Item = &'static str;

    fn search(&self, _query: &str) -> std::io::Result<Vec<RankedHit<Self::Item>>> {
        Ok(self
            .hits
            .iter()
            .enumerate()
            .map(|(rank, (score, item))| RankedHit::new(rank, *score, *item))
            .collect())
    }
}

#[test]
fn federated_search_merges_and_reranks_deterministically() -> Result<(), Box<dyn std::error::Error>>
{
    let state = TempDir::new("federated")?;
    let repo_a = repository("repo.a")?;
    let repo_b = repository("repo.b")?;
    let repo_missing = repository("repo.missing")?;

    let mut manifest = WorkspaceManifest::create(state.path(), "team")?;
    manifest.add_repository(repo_a.clone())?;
    manifest.add_repository(repo_b.clone())?;
    manifest.add_repository(repo_missing.clone())?;

    let result = federated_search(&manifest, "query", |repository| {
        if *repository == repo_a {
            Ok(Some(FakeSource {
                hits: vec![(0.9, "a-high"), (0.9, "a-high-2"), (0.5, "a-low")],
            }))
        } else if *repository == repo_b {
            Ok(Some(FakeSource {
                hits: vec![(0.9, "b-high")],
            }))
        } else {
            Ok(None)
        }
    })?;

    let items: Vec<&str> = result.hits.iter().map(|hit| hit.item).collect();
    assert_eq!(items, vec!["a-high", "a-high-2", "b-high", "a-low"]);
    assert_eq!(result.hits[0].repository, repo_a);
    assert_eq!(result.hits[0].repository_rank, 0);
    assert_eq!(result.hits[1].repository, repo_a);
    assert_eq!(result.hits[1].repository_rank, 1);
    assert_eq!(result.hits[2].repository, repo_b);
    assert_eq!(result.unresolved, vec![repo_missing]);
    Ok(())
}

#[test]
fn federated_search_reports_hard_open_and_search_failures() -> Result<(), Box<dyn std::error::Error>>
{
    let state = TempDir::new("federated-error")?;
    let repo_a = repository("repo.a")?;
    let mut manifest = WorkspaceManifest::create(state.path(), "team")?;
    manifest.add_repository(repo_a)?;

    let outcome = federated_search::<_, FakeSource>(&manifest, "query", |_repository| {
        Err(std::io::Error::other("backend unavailable"))
    });
    assert!(outcome.is_err());
    Ok(())
}
