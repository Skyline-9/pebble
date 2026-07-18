//! Local model-free retrieval and cited evidence packets.

mod candidate;
mod contradiction;
mod packet;
mod ranking;
mod request;
mod trace;

use thiserror::Error;

use crate::domain::EvidencePacket;
use crate::error::DomainError;
use crate::index::{GenerationReader, IndexError};
use crate::vectors::FINGERPRINT_LEN;

pub use request::SearchRequest;
pub use trace::{OmittedCandidate, QueryTrace, TraceCandidate};

/// A caller-supplied query embedding paired with the model fingerprint that
/// produced it.
///
/// Supplying this is entirely optional. When present, and when the pinned
/// generation has a validated vector index built with a matching
/// fingerprint, [`RetrievalEngine::search_with_embedding`] folds bounded
/// cosine-similarity candidates into the existing weighted reciprocal-rank
/// fusion. When absent, retrieval behavior is byte-for-byte identical to the
/// model-free path.
pub struct QueryEmbedding {
    vector: Vec<f32>,
    fingerprint: [u8; FINGERPRINT_LEN],
}

impl QueryEmbedding {
    /// Construct a validated query embedding.
    ///
    /// # Errors
    ///
    /// Returns an error when the vector is empty or contains a non-finite
    /// value.
    pub fn new(
        vector: Vec<f32>,
        fingerprint: [u8; FINGERPRINT_LEN],
    ) -> Result<Self, RetrievalError> {
        if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
            return Err(RetrievalError::InvalidRequest(
                "query embedding must be a nonempty vector of finite values".to_owned(),
            ));
        }
        Ok(Self {
            vector,
            fingerprint,
        })
    }

    pub(super) fn vector(&self) -> &[f32] {
        &self.vector
    }

    pub(super) const fn fingerprint(&self) -> [u8; FINGERPRINT_LEN] {
        self.fingerprint
    }
}

/// Failure while validating or executing a local retrieval.
#[derive(Debug, Error)]
pub enum RetrievalError {
    /// A request field is invalid.
    #[error("invalid retrieval request: {0}")]
    InvalidRequest(String),
    /// The pinned index generation could not be queried.
    #[error(transparent)]
    Index(#[from] IndexError),
    /// A stable citation or packet value failed validation.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// A local trace filesystem operation failed.
    #[error("retrieval trace filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// A bounded local trace could not be encoded.
    #[error("retrieval trace encoding failed: {0}")]
    Json(#[from] serde_json::Error),
    /// A trace record or trace file reached its fixed size ceiling.
    #[error("retrieval trace size bound exceeded")]
    TraceBoundExceeded,
    /// Candidate generation exceeded the fixed global identity ceiling.
    #[error("retrieval candidate bound exceeded (maximum {maximum})")]
    CandidateBoundExceeded {
        /// Maximum distinct candidate identities accepted by one search.
        maximum: usize,
    },
    /// The process-local concurrent append lock was poisoned.
    #[error("retrieval trace append lock was poisoned")]
    TraceLockPoisoned,
}

/// Packet plus local observability data from one pinned generation search.
pub struct SearchResponse {
    packet: EvidencePacket,
    trace: QueryTrace,
    estimated_tokens: u32,
}

impl SearchResponse {
    /// Return the cited evidence packet.
    #[must_use]
    pub const fn packet(&self) -> &EvidencePacket {
        &self.packet
    }

    /// Return the bounded trace, which contains no query or source text.
    #[must_use]
    pub const fn trace(&self) -> &QueryTrace {
        &self.trace
    }

    /// Return the conservative estimated packet token use.
    #[must_use]
    pub const fn estimated_tokens(&self) -> u32 {
        self.estimated_tokens
    }
}

/// Model-free search over one already pinned immutable generation.
pub struct RetrievalEngine<'generation> {
    reader: &'generation GenerationReader,
}

impl<'generation> RetrievalEngine<'generation> {
    /// Bind retrieval to exactly one immutable generation reader.
    #[must_use]
    pub const fn new(reader: &'generation GenerationReader) -> Self {
        Self { reader }
    }

    /// Retrieve, resolve, diversify, and budget a cited evidence packet.
    ///
    /// This method performs no answer synthesis and no network operation.
    ///
    /// # Errors
    ///
    /// Returns an error when the pinned indexes cannot be read, citations
    /// cannot be validated, or an explicitly requested trace cannot be safely
    /// appended.
    pub fn search(&self, request: SearchRequest) -> Result<SearchResponse, RetrievalError> {
        self.search_with_embedding(request, None)
    }

    /// Retrieve, resolve, diversify, and budget a cited evidence packet,
    /// optionally folding vector candidates from `query_embedding` into the
    /// existing weighted reciprocal-rank fusion.
    ///
    /// When `query_embedding` is `None`, this method is byte-for-byte
    /// equivalent to [`Self::search`]. This method performs no answer
    /// synthesis and no network operation.
    ///
    /// # Errors
    ///
    /// Returns an error when the pinned indexes cannot be read, citations
    /// cannot be validated, or an explicitly requested trace cannot be safely
    /// appended.
    pub fn search_with_embedding(
        &self,
        request: SearchRequest,
        query_embedding: Option<&QueryEmbedding>,
    ) -> Result<SearchResponse, RetrievalError> {
        let generated = candidate::generate(self.reader, &request, query_embedding)?;
        let active_scorers = generated
            .pools
            .iter()
            .map(|pool| pool.scorer.to_owned())
            .collect::<Vec<_>>();
        let ranked = ranking::fuse(&generated.pools);
        let built = packet::build(self.reader, &request, &ranked)?;
        let diagnostics = built
            .packet
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code.clone())
            .collect();
        let mut omitted = generated.omitted;
        omitted.extend(built.omitted);
        let trace = QueryTrace::new(
            self.reader.id().as_str().to_owned(),
            active_scorers,
            built.selected,
            omitted,
            diagnostics,
        );
        if let Some(path) = request.trace_path {
            trace::append(&path, &trace)?;
        }
        Ok(SearchResponse {
            packet: built.packet,
            trace,
            estimated_tokens: built.estimated_tokens,
        })
    }
}
