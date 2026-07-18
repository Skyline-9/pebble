//! Immutable generation-local evidence graph storage.

mod building_boundary;
mod compiler;
mod cross_index;
mod current;
mod generation;
mod generation_fs;
mod generation_races;
mod graph;
mod graph_transaction;
mod graph_writer;
mod lexical;
#[cfg(all(test, not(any(target_os = "macos", target_os = "ios"))))]
mod lexical_races;
mod lexical_reader;
mod pinned_directory;
mod pinned_directory_platform;
mod retrieval;
mod schema;
mod schema_shape;

use std::fmt;

use thiserror::Error;

use crate::error::DomainError;
use crate::repository::RepositoryError;

pub use compiler::{CompilerFault, RepositoryCompiler};
pub use generation::{GenerationBuilder, GenerationReader, SealedGeneration};
pub use graph::{EdgeTarget, GraphCounts, GraphEdge, GraphEdgeKind, GraphReadLimit, GraphReader};
pub use graph_transaction::GraphTransaction;
pub use graph_writer::GraphWriter;
pub use lexical::LexicalWriter;
pub use lexical_reader::{LexicalHit, LexicalReader};
pub(crate) use retrieval::RetrievalEntity;

/// Failure while building, validating, publishing, or reading an index generation.
#[derive(Debug, Error)]
pub enum IndexError {
    /// A `SQLite` operation failed.
    #[error("SQLite graph operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A Tantivy lexical index operation failed.
    #[error("Tantivy lexical operation failed: {0}")]
    Tantivy(#[from] tantivy::TantivyError),
    /// A local filesystem operation failed.
    #[error("index filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// A stable domain value failed validation.
    #[error("index domain value is invalid: {0}")]
    Domain(#[from] DomainError),
    /// Repository snapshot or restricted Git access failed.
    #[error("repository compilation input failed: {0}")]
    Repository(#[from] RepositoryError),
    /// An idempotent graph identity was reused with a different payload.
    #[error("index graph identity conflicts with its existing payload: {0}")]
    Conflict(&'static str),
    /// Disposable projection state is missing or corrupt and must be rebuilt.
    #[error("index rebuild required: {0}")]
    RebuildRequired(String),
    /// A test or operator-requested compiler fault interrupted a disposable build.
    #[error("index compilation interrupted at injected fault: {0}")]
    InjectedFault(&'static str),
    /// A query exceeded the fixed lexical input byte bound.
    #[error("lexical query contains {actual} bytes, exceeding the {maximum}-byte limit")]
    QueryTooLarge {
        /// Actual query size in UTF-8 bytes.
        actual: usize,
        /// Maximum accepted query size in UTF-8 bytes.
        maximum: usize,
    },
    /// Tokenization produced more distinct query terms than one Boolean query may contain.
    #[error("lexical query exceeds the {maximum}-term limit")]
    TooManyQueryTerms {
        /// Maximum accepted number of unique terms.
        maximum: usize,
    },
    /// The requested generation ID already has an inert building path.
    ///
    /// Callers must allocate a fresh generation ID. Pebble never deletes or
    /// reuses an existing building path because it may be unowned.
    #[error("incomplete generation build already exists for {generation}; allocate a fresh ID")]
    IncompleteBuild {
        /// Generation ID whose building path is already reserved.
        generation: String,
    },
}

impl IndexError {
    pub(super) fn rebuild(message: impl Into<String>) -> Self {
        Self::RebuildRequired(message.into())
    }

    pub(super) fn into_rebuild(error: impl fmt::Display) -> Self {
        Self::rebuild(error.to_string())
    }
}
