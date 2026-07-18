//! Federated search across the repositories listed in one workspace.

use std::cmp::Ordering;
use std::io;

use crate::domain::RepositoryId;

use super::manifest::WorkspaceManifest;

/// The knowledge scope targeted by one search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSearchScope {
    /// Search only the current repository.
    Repository(RepositoryId),
    /// Search every present repository named in one workspace manifest.
    Workspace(String),
    /// Search personal knowledge only.
    Personal,
}

/// One per-repository search result, ranked before cross-repository merge.
#[derive(Clone, Debug)]
pub struct RankedHit<T> {
    /// Repository-local rank, where zero is the best match.
    pub rank: usize,
    /// Repository-local relevance score, where a higher value is better.
    pub score: f64,
    /// The repository-specific result payload.
    pub item: T,
}

impl<T> RankedHit<T> {
    /// Construct one ranked per-repository hit.
    #[must_use]
    pub const fn new(rank: usize, score: f64, item: T) -> Self {
        Self { rank, score, item }
    }
}

/// A per-repository search backend opened for one present repository.
///
/// The CLI or service integration layer implements this trait by wrapping
/// the real `RetrievalEngine` bound to that repository's pinned generation
/// reader. This module stays independent of the retrieval engine's types.
pub trait RepositoryResultSource {
    /// The repository-specific result payload produced by one search.
    type Item;

    /// Run `query` against this open per-repository backend.
    ///
    /// # Errors
    ///
    /// Returns an error when the per-repository backend cannot be queried.
    fn search(&self, query: &str) -> io::Result<Vec<RankedHit<Self::Item>>>;
}

/// One merged cross-repository hit, tagged with its source repository.
#[derive(Clone, Debug)]
pub struct WorkspaceHit<T> {
    /// Repository that produced this result.
    pub repository: RepositoryId,
    /// Repository-local rank this hit held before cross-repository merge.
    pub repository_rank: usize,
    /// Repository-local relevance score used for cross-repository ranking.
    pub score: f64,
    /// The repository-specific result payload.
    pub item: T,
}

/// The merged outcome of one federated workspace search.
#[derive(Clone, Debug)]
pub struct WorkspaceSearchResult<T> {
    /// Merged hits ordered by descending score with deterministic ties.
    pub hits: Vec<WorkspaceHit<T>>,
    /// Workspace member repositories not currently present on this machine.
    pub unresolved: Vec<RepositoryId>,
}

/// Run one query against every present repository in a workspace manifest.
///
/// `open` returns the per-repository search backend for a member
/// repository, or `Ok(None)` when that repository is not currently
/// registered on this machine. Results from every present repository are
/// merged and re-ranked by score, with ties broken first by repository ID
/// and then by each hit's existing per-repository rank. Repositories that
/// `open` reports absent are collected as `unresolved` rather than being
/// silently dropped or causing a hard failure.
///
/// # Errors
///
/// Returns an error when opening or searching a present repository fails.
pub fn federated_search<F, R>(
    manifest: &WorkspaceManifest,
    query: &str,
    open: F,
) -> io::Result<WorkspaceSearchResult<R::Item>>
where
    F: Fn(&RepositoryId) -> io::Result<Option<R>>,
    R: RepositoryResultSource,
{
    let mut hits = Vec::new();
    let mut unresolved = Vec::new();
    for repository in manifest.repositories() {
        let Some(source) = open(repository)? else {
            unresolved.push(repository.clone());
            continue;
        };
        for ranked in source.search(query)? {
            hits.push(WorkspaceHit {
                repository: repository.clone(),
                repository_rank: ranked.rank,
                score: ranked.score,
                item: ranked.item,
            });
        }
    }
    hits.sort_by(merge_order);
    Ok(WorkspaceSearchResult { hits, unresolved })
}

fn merge_order<T>(left: &WorkspaceHit<T>, right: &WorkspaceHit<T>) -> Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.repository.cmp(&right.repository))
        .then_with(|| left.repository_rank.cmp(&right.repository_rank))
}
