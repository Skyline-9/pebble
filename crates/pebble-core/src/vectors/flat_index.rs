//! Bounded brute-force cosine-similarity search over one sealed vector
//! generation.
//!
//! Dataset sizes are single-repository scale, so an exact linear scan is
//! used instead of an approximate nearest-neighbor structure. Reads use
//! bounded seeks over the sealed file rather than loading every row into
//! memory at once.

use std::cmp::Ordering;
use std::io;
use std::path::Path;

use super::format::{FINGERPRINT_LEN, VectorFileReader};

/// Largest `k` accepted by [`FlatVectorIndex::top_k`].
pub const MAX_TOP_K: usize = 1_024;

/// Read-only bounded brute-force cosine-similarity index over one sealed
/// vector generation.
pub struct FlatVectorIndex {
    reader: VectorFileReader,
}

impl FlatVectorIndex {
    /// Open and fully validate one sealed vector generation for search.
    ///
    /// # Errors
    ///
    /// Returns an error when the generation is missing, corrupt, truncated,
    /// or was built with a model fingerprint other than
    /// `expected_fingerprint`.
    pub fn open(
        vector_path: &Path,
        ids_path: &Path,
        expected_fingerprint: [u8; FINGERPRINT_LEN],
    ) -> io::Result<Self> {
        Ok(Self {
            reader: VectorFileReader::open(vector_path, ids_path, expected_fingerprint)?,
        })
    }

    /// Return the fixed embedding dimension of this generation.
    #[must_use]
    pub const fn dimension(&self) -> usize {
        self.reader.dimension()
    }

    /// Return the validated row count of this generation.
    #[must_use]
    pub const fn row_count(&self) -> u64 {
        self.reader.row_count()
    }

    /// Return up to `k` nearest rows to `query` by cosine similarity,
    /// paired with the entity ID stored for each row.
    ///
    /// Ties break deterministically by ascending stable entity ID.
    ///
    /// # Errors
    ///
    /// Returns an error when `query` does not match the fixed dimension,
    /// `k` is zero or exceeds [`MAX_TOP_K`], or a row cannot be read.
    pub fn top_k(&self, query: &[f32], k: usize) -> io::Result<Vec<(String, f32)>> {
        if query.len() != self.dimension() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "query embedding does not match the index dimension",
            ));
        }
        if k == 0 || k > MAX_TOP_K {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "top_k bound must be between 1 and the maximum supported result count",
            ));
        }
        let query_norm = norm(query);
        let mut scored = Vec::new();
        for ordinal in 0..self.reader.row_count() {
            let vector = self.reader.read_row(ordinal)?;
            let score = cosine_similarity(query, query_norm, &vector);
            let Some(entity_id) = self.reader.entity_id(ordinal) else {
                continue;
            };
            scored.push((entity_id.to_owned(), score));
        }
        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        scored.truncate(k);
        Ok(scored)
    }
}

fn norm(vector: &[f32]) -> f32 {
    vector.iter().map(|value| value * value).sum::<f32>().sqrt()
}

fn cosine_similarity(query: &[f32], query_norm: f32, row: &[f32]) -> f32 {
    if query_norm <= f32::EPSILON {
        return 0.0;
    }
    let row_norm = norm(row);
    if row_norm <= f32::EPSILON {
        return 0.0;
    }
    let dot = query
        .iter()
        .zip(row)
        .map(|(left, right)| left * right)
        .sum::<f32>();
    dot / (query_norm * row_norm)
}
