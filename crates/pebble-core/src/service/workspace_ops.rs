//! Multi-repository workspace management and federated search.

use std::io;
use std::path::PathBuf;

use serde::Serialize;

use crate::domain::RepositoryId;
use crate::index::GenerationReader;
use crate::retrieval::{RetrievalEngine, SearchRequest};
use crate::workspace::{
    RankedHit, RepositoryResultSource, WorkspaceError, WorkspaceManifest, federated_search,
};

use super::{PebbleService, ServiceError};

/// One workspace's declared name and member repositories.
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceSummary {
    /// Workspace's validated name.
    pub name: String,
    /// Repositories currently listed by the workspace.
    pub repositories: Vec<RepositoryId>,
}

impl From<&WorkspaceManifest> for WorkspaceSummary {
    fn from(manifest: &WorkspaceManifest) -> Self {
        Self {
            name: manifest.name().to_owned(),
            repositories: manifest.repositories().to_vec(),
        }
    }
}

/// One merged cross-repository search hit.
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceSearchHit {
    /// Repository that produced this hit.
    pub repository: RepositoryId,
    /// Cross-repository relevance score, where a higher value is better.
    pub score: f64,
    /// Repository-relative source path.
    pub path: String,
    /// First cited line, starting at one.
    pub start_line: u32,
    /// Last cited line, inclusive.
    pub end_line: u32,
    /// Source excerpt bounded by the requested packet budget.
    pub content: String,
}

/// Merged outcome of one federated workspace search.
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceSearchOutcome {
    /// Merged hits ordered by descending score.
    pub hits: Vec<WorkspaceSearchHit>,
    /// Workspace member repositories not currently present on this machine.
    pub unresolved: Vec<RepositoryId>,
}

impl PebbleService {
    /// Create a new empty named workspace.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error for an unsafe or already-used name,
    /// or an operational error when the manifest cannot be written.
    pub fn workspace_create(&self, name: &str) -> Result<WorkspaceSummary, ServiceError> {
        let manifest = WorkspaceManifest::create(self.state_root(), name).map_err(map_workspace)?;
        Ok(WorkspaceSummary::from(&manifest))
    }

    /// Add a registered repository to a named workspace.
    ///
    /// # Errors
    ///
    /// Returns an unavailable-evidence error when the repository is not
    /// registered, a classified usage error for an unknown workspace, or an
    /// operational error when the manifest cannot be written.
    pub fn workspace_add_repository(
        &self,
        name: &str,
        repository: &RepositoryId,
    ) -> Result<WorkspaceSummary, ServiceError> {
        self.registration(repository)?;
        let mut manifest =
            WorkspaceManifest::load(self.state_root(), name).map_err(map_workspace)?;
        manifest
            .add_repository(repository.clone())
            .map_err(map_workspace)?;
        Ok(WorkspaceSummary::from(&manifest))
    }

    /// List the names of every workspace.
    ///
    /// # Errors
    ///
    /// Returns an operational error when workspace storage cannot be read.
    pub fn workspace_list(&self) -> Result<Vec<String>, ServiceError> {
        WorkspaceManifest::list(self.state_root()).map_err(map_workspace)
    }

    /// Run one query against every present repository in a named workspace,
    /// merging per-repository results by descending relevance score.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error for an unknown workspace or invalid
    /// query parameters, or an operational error when a present repository
    /// cannot be searched.
    pub fn workspace_search(
        &self,
        name: &str,
        query: &str,
        budget_tokens: u32,
        max_results: usize,
    ) -> Result<WorkspaceSearchOutcome, ServiceError> {
        let manifest = WorkspaceManifest::load(self.state_root(), name).map_err(map_workspace)?;
        let result = federated_search(&manifest, query, |repository| {
            Ok(self.open_repository_source(repository, budget_tokens, max_results))
        })
        .map_err(ServiceError::operational)?;
        let hits = result
            .hits
            .into_iter()
            .map(|hit| WorkspaceSearchHit {
                repository: hit.repository,
                score: hit.score,
                path: hit.item.path,
                start_line: hit.item.start_line,
                end_line: hit.item.end_line,
                content: hit.item.content,
            })
            .collect();
        Ok(WorkspaceSearchOutcome {
            hits,
            unresolved: result.unresolved,
        })
    }

    fn open_repository_source(
        &self,
        repository: &RepositoryId,
        budget_tokens: u32,
        max_results: usize,
    ) -> Option<RepoSource> {
        if self.registration(repository).is_err() {
            return None;
        }
        GenerationReader::open_current(&self.layout.generations(repository))
            .ok()
            .map(|reader| RepoSource {
                reader,
                trace_path: self.repository_root(repository).join("traces.jsonl"),
                budget_tokens,
                max_results,
                repository: repository.clone(),
            })
    }
}

struct RepoSource {
    reader: GenerationReader,
    trace_path: PathBuf,
    budget_tokens: u32,
    max_results: usize,
    repository: RepositoryId,
}

struct RepoSearchItem {
    path: String,
    start_line: u32,
    end_line: u32,
    content: String,
}

impl RepositoryResultSource for RepoSource {
    type Item = RepoSearchItem;

    fn search(&self, query: &str) -> io::Result<Vec<RankedHit<Self::Item>>> {
        let request = SearchRequest::new(query.to_owned())
            .and_then(|request| request.with_repository(self.repository.as_str().to_owned()))
            .and_then(|request| request.with_budget_tokens(self.budget_tokens))
            .and_then(|request| request.with_max_results(self.max_results))
            .and_then(|request| request.with_trace_path(&self.trace_path))
            .map_err(io::Error::other)?;
        let response = RetrievalEngine::new(&self.reader)
            .search(request)
            .map_err(io::Error::other)?;
        Ok(response
            .packet()
            .items()
            .iter()
            .enumerate()
            .map(|(rank, item)| {
                let score = item
                    .score_explanations
                    .iter()
                    .map(|explanation| f64::from(explanation.score))
                    .sum::<f64>();
                RankedHit::new(
                    rank,
                    score,
                    RepoSearchItem {
                        path: item.citation.path().to_owned(),
                        start_line: item.citation.start_line(),
                        end_line: item.citation.end_line(),
                        content: item.content.clone(),
                    },
                )
            })
            .collect())
    }
}

fn map_workspace(error: WorkspaceError) -> ServiceError {
    match error {
        WorkspaceError::InvalidName(_)
        | WorkspaceError::AlreadyExists(_)
        | WorkspaceError::NotFound(_)
        | WorkspaceError::InvalidManifest(_) => ServiceError::usage(error),
        WorkspaceError::Io(_) => ServiceError::operational(error),
    }
}
