//! Bounded flat vector index generations keyed by embedding model
//! fingerprint.
//!
//! Pebble's default retrieval path is model-free. When a local embedding
//! runtime is available, the index compiler can additionally build one
//! sealed flat vector generation per index generation, and the retrieval
//! engine can fold its bounded brute-force cosine-similarity candidates into
//! the existing weighted reciprocal-rank fusion. Both are entirely optional;
//! nothing in this crate depends on a concrete embedding runtime.

use std::io;

/// Bounded brute-force cosine-similarity search over one sealed vector
/// generation.
pub mod flat_index;
/// Sealed flat-vector generation file format and its bounded streaming I/O.
pub mod format;

pub use flat_index::FlatVectorIndex;
pub use format::FINGERPRINT_LEN;

/// Minimal local text-embedding contract an optional embedding runtime
/// implements.
///
/// `pebble-core` depends only on this trait, never on a concrete embedding
/// crate, so the index compiler and retrieval engine remain fully usable
/// with no embedding runtime installed.
pub trait TextEmbedder {
    /// Embed one text input into a dense vector of [`Self::dimension`]
    /// values.
    ///
    /// # Errors
    ///
    /// Returns an error when local inference fails.
    fn embed_one(&self, text: &str) -> io::Result<Vec<f32>>;

    /// Return the fixed embedding dimension produced by this embedder.
    fn dimension(&self) -> usize;

    /// Return the stable fingerprint identifying this embedding model and
    /// its configuration (for example repository, revision, pooling,
    /// normalization, and runtime version).
    fn fingerprint(&self) -> [u8; FINGERPRINT_LEN];
}
