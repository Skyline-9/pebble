#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Stable domain contracts shared by Pebble application layers.

/// Domain identities, revisions, citations, and evidence packets.
pub mod domain;
/// Consent-gated local embedding models and inference.
pub mod embeddings;
/// Errors returned by stable domain contracts.
pub mod error;
/// Immutable generation-local evidence graph storage.
pub mod index;
/// Bounded chunks and packaged polyglot structural extraction.
pub mod ingestion;
/// Managed living-knowledge notes, claim state, and update packets.
pub mod knowledge;
/// Personal knowledge notes and promotion.
pub mod personal;
/// Repository-local state contracts.
pub mod repository;
/// Local model-free retrieval and cited evidence packets.
pub mod retrieval;
/// Model-free application operations over secure local state.
pub mod service;
/// Bounded flat vector index generations.
pub mod vectors;
/// Bounded repository watching, event coalescing, and reconciliation.
pub mod watcher;
/// Multi-repository workspace manifests and federated search.
pub mod workspace;

/// Current Pebble product version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
