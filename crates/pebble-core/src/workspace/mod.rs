//! Multi-repository workspace manifests and federated search.

/// Named multi-repository workspace manifests and their durable storage.
pub mod manifest;
/// Federated cross-repository search dispatch and deterministic merging.
pub mod search;

pub use manifest::{WorkspaceError, WorkspaceManifest};
pub use search::{
    RankedHit, RepositoryResultSource, WorkspaceHit, WorkspaceSearchResult, WorkspaceSearchScope,
    federated_search,
};
