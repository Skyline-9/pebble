//! Grounded retrieval evidence contracts.

use serde::{Deserialize, Deserializer, Serialize};

use super::{RepositoryId, WorktreeRevision};
use crate::error::DomainError;

/// Smallest supported evidence budget in model tokens.
pub const MIN_EVIDENCE_TOKENS: u32 = 1_000;
/// Largest supported evidence budget in model tokens.
pub const MAX_EVIDENCE_TOKENS: u32 = 32_000;
/// Default evidence budget in model tokens.
pub const DEFAULT_EVIDENCE_TOKENS: u32 = 6_000;

/// A resolvable one-based inclusive source range.
///
/// Invariant-bearing values are exposed only through read-only accessors.
///
/// ```compile_fail
/// use pebble_core::domain::{Citation, RepositoryId, WorktreeRevision};
///
/// let repository = RepositoryId::try_from("acme-pebble".to_owned())?;
/// let revision = WorktreeRevision::clean("0123456789abcdef")?;
/// let mut citation = Citation::new(repository, revision, "src/lib.rs", 1, 8)?;
/// citation.path = "../outside.rs".to_owned();
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Citation {
    /// Canonical repository containing the cited source.
    repository: RepositoryId,
    /// Indexed worktree revision containing the cited source.
    revision: WorktreeRevision,
    /// Normalized repository-relative slash path.
    path: String,
    /// First cited line, starting at one.
    start_line: u32,
    /// Last cited line, inclusive.
    end_line: u32,
}

#[derive(Deserialize)]
struct CitationWire {
    repository: RepositoryId,
    revision: WorktreeRevision,
    path: String,
    start_line: u32,
    end_line: u32,
}

impl<'de> Deserialize<'de> for Citation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CitationWire::deserialize(deserializer)?;
        Self::new(
            wire.repository,
            wire.revision,
            wire.path,
            wire.start_line,
            wire.end_line,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl Citation {
    /// Construct a validated citation.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty revision, a non-normalized path, or an
    /// invalid line range.
    pub fn new(
        repository: RepositoryId,
        revision: WorktreeRevision,
        path: impl Into<String>,
        start_line: u32,
        end_line: u32,
    ) -> Result<Self, DomainError> {
        let path = path.into();
        validate_relative_path(&path)?;
        if start_line == 0 || end_line < start_line {
            return Err(DomainError::InvalidLineRange);
        }
        Ok(Self {
            repository,
            revision,
            path,
            start_line,
            end_line,
        })
    }

    /// Return the repository containing the cited source.
    #[must_use]
    pub const fn repository(&self) -> &RepositoryId {
        &self.repository
    }

    /// Return the indexed worktree revision containing the cited source.
    #[must_use]
    pub const fn revision(&self) -> &WorktreeRevision {
        &self.revision
    }

    /// Return the normalized repository-relative slash path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the first cited line, starting at one.
    #[must_use]
    pub const fn start_line(&self) -> u32 {
        self.start_line
    }

    /// Return the last cited line, inclusive.
    #[must_use]
    pub const fn end_line(&self) -> u32 {
        self.end_line
    }
}

/// Contribution of one retrieval scorer to an evidence result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ScoreExplanation {
    /// Stable name of the scorer.
    pub scorer: String,
    /// Normalized score assigned by that scorer.
    pub score: f32,
    /// Human-readable reason for the score.
    pub explanation: String,
}

/// One source excerpt returned to a calling agent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EvidenceItem {
    /// Source range grounding this excerpt.
    pub citation: Citation,
    /// Source text bounded by the packet budget.
    pub content: String,
    /// Per-scorer reasons for including the excerpt.
    pub score_explanations: Vec<ScoreExplanation>,
}

/// Nonfatal retrieval information included with an evidence packet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceDiagnostic {
    /// Stable machine-readable diagnostic code.
    pub code: String,
    /// Human-readable diagnostic message.
    pub message: String,
}

/// Token-bounded evidence and diagnostics returned to a calling agent.
///
/// The packet deliberately contains no synthesized answer. Answer generation
/// remains the responsibility of the calling agent.
///
/// ```compile_fail
/// use pebble_core::domain::EvidencePacket;
///
/// let mut packet = EvidencePacket::new(1_000, Vec::new(), Vec::new())?;
/// packet.budget_tokens = 999;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// ```compile_fail
/// use pebble_core::domain::EvidencePacket;
///
/// let packet = EvidencePacket {
///     budget_tokens: 999,
///     items: Vec::new(),
///     diagnostics: Vec::new(),
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EvidencePacket {
    /// Configured maximum packet size in model tokens.
    budget_tokens: u32,
    /// Grounded source excerpts selected by retrieval.
    items: Vec<EvidenceItem>,
    /// Nonfatal conditions encountered during retrieval.
    diagnostics: Vec<EvidenceDiagnostic>,
}

#[derive(Deserialize)]
struct EvidencePacketWire {
    budget_tokens: u32,
    items: Vec<EvidenceItem>,
    diagnostics: Vec<EvidenceDiagnostic>,
}

impl<'de> Deserialize<'de> for EvidencePacket {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = EvidencePacketWire::deserialize(deserializer)?;
        Self::new(wire.budget_tokens, wire.items, wire.diagnostics)
            .map_err(serde::de::Error::custom)
    }
}

impl EvidencePacket {
    /// Construct a packet with a validated inclusive token budget.
    ///
    /// # Errors
    ///
    /// Returns an error when `budget_tokens` is outside 1,000 through 32,000.
    pub fn new(
        budget_tokens: u32,
        items: Vec<EvidenceItem>,
        diagnostics: Vec<EvidenceDiagnostic>,
    ) -> Result<Self, DomainError> {
        if !(MIN_EVIDENCE_TOKENS..=MAX_EVIDENCE_TOKENS).contains(&budget_tokens) {
            return Err(DomainError::InvalidEvidenceBudget {
                minimum: MIN_EVIDENCE_TOKENS,
                maximum: MAX_EVIDENCE_TOKENS,
            });
        }
        Ok(Self {
            budget_tokens,
            items,
            diagnostics,
        })
    }

    /// Return the configured maximum packet size in model tokens.
    #[must_use]
    pub const fn budget_tokens(&self) -> u32 {
        self.budget_tokens
    }

    /// Return the grounded source excerpts selected by retrieval.
    #[must_use]
    pub fn items(&self) -> &[EvidenceItem] {
        &self.items
    }

    /// Return the nonfatal conditions encountered during retrieval.
    #[must_use]
    pub fn diagnostics(&self) -> &[EvidenceDiagnostic] {
        &self.diagnostics
    }
}

fn validate_relative_path(path: &str) -> Result<(), DomainError> {
    let invalid = path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."));
    if invalid {
        return Err(DomainError::InvalidCitationPath);
    }
    Ok(())
}
