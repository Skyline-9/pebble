//! Consent-gated local Hugging Face embedding models and inference.
//!
//! Pebble never downloads or executes a model without a caller's explicit,
//! logged consent. Every installed model is described by a pinned,
//! checksum-verified [`ModelManifest`](crate::embeddings::ModelManifest) (see
//! the `manifest` module), whose install disclosure and approval are tracked by
//! [`ConsentState`](crate::embeddings::ConsentState) (see the `consent`
//! module), stored on disk by [`ModelStore`](crate::embeddings::ModelStore)
//! (see the `model` module), and loaded for genuine local CPU inference by
//! [`EmbeddingRuntime`](crate::embeddings::EmbeddingRuntime) (see the `runtime`
//! module). Without an installed model, semantic scoring is simply
//! absent; lexical, structural, and graph retrieval remain unaffected.

mod consent;
mod manifest;
mod model;
mod runtime;

pub use consent::{ConsentState, render_disclosure};
pub use manifest::{ManifestFile, ModelManifest, PoolingStrategy};
pub use model::ModelStore;
pub use runtime::{EmbeddingRuntime, MAX_EMBED_BATCH_SIZE, MAX_EMBED_TEXT_TOKENS};

use thiserror::Error;

/// Failure installing, storing, or running a local embedding model.
#[derive(Debug, Error)]
pub enum EmbeddingError {
    /// A filesystem operation failed.
    #[error("embedding model I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Manifest, consent-state, or configuration JSON could not be
    /// (de)serialized.
    #[error("embedding model metadata failed to (de)serialize: {0}")]
    Json(#[from] serde_json::Error),
    /// A manifest, file name, or checksum failed validation.
    #[error("invalid embedding model manifest: {0}")]
    InvalidManifest(String),
    /// A downloaded or stored file's checksum did not match its manifest.
    #[error("checksum mismatch for {file}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Manifest-declared file name that failed verification.
        file: String,
        /// Checksum recorded in the manifest.
        expected: String,
        /// Checksum computed from the downloaded or stored bytes.
        actual: String,
    },
    /// The requested model is not installed.
    #[error("embedding model {0} is not installed")]
    ModelNotFound(String),
    /// A candle tensor or model operation failed.
    #[error("embedding inference failed: {0}")]
    Candle(#[from] candle_core::Error),
    /// A tokenizer construction or encoding operation failed.
    #[error("embedding tokenizer failed: {0}")]
    Tokenizer(tokenizers::Error),
    /// A batch exceeded the runtime's bounded input count.
    #[error("embedding batch of {actual} texts exceeds the {limit}-text limit")]
    BatchTooLarge {
        /// Number of texts submitted.
        actual: usize,
        /// Maximum accepted number of texts per call.
        limit: usize,
    },
}
