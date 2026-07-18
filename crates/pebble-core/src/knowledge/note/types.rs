//! Claim status, review state, and anchor types recorded on a managed note.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::knowledge::KnowledgeError;

/// Resolvability state of a managed claim's citations against indexed evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimStatus {
    /// Citations resolve against the recorded revisions.
    Current,
    /// An inferred or transitively linked anchor may no longer be accurate.
    Stale,
    /// A directly affected anchor requires agent-generated replacement prose.
    PendingUpdate,
    /// An explicit anchor no longer resolves.
    Broken,
}

impl ClaimStatus {
    /// Return the documented lowercase frontmatter token for this status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::PendingUpdate => "pending_update",
            Self::Broken => "broken",
        }
    }
}

impl TryFrom<&str> for ClaimStatus {
    type Error = KnowledgeError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "current" => Ok(Self::Current),
            "stale" => Ok(Self::Stale),
            "pending_update" => Ok(Self::PendingUpdate),
            "broken" => Ok(Self::Broken),
            other => Err(KnowledgeError::InvalidClaimStatus(other.to_owned())),
        }
    }
}

/// Review provenance of a claim's managed prose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewState {
    /// Structural patch validation produced or last touched this claim.
    AgentGenerated,
    /// A person reviewed and accepted this claim's prose.
    HumanVerified,
}

impl ReviewState {
    /// Return the documented lowercase frontmatter token for this review state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentGenerated => "agent_generated",
            Self::HumanVerified => "human_verified",
        }
    }
}

impl TryFrom<&str> for ReviewState {
    type Error = KnowledgeError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "agent_generated" => Ok(Self::AgentGenerated),
            "human_verified" => Ok(Self::HumanVerified),
            other => Err(KnowledgeError::InvalidReviewState(other.to_owned())),
        }
    }
}

/// One code-dependency anchor recorded on a managed claim.
#[derive(Clone, Debug, PartialEq)]
pub enum Anchor {
    /// An author-declared, authoritative anchor.
    Explicit {
        /// Canonical repository ID containing the anchor.
        repo: String,
        /// Repository-relative anchor path.
        path: String,
        /// Anchored symbol name.
        symbol: String,
    },
    /// A Pebble-discovered anchor with confidence and provenance.
    Inferred {
        /// Canonical repository ID containing the anchor.
        repo: String,
        /// Repository-relative anchor path.
        path: String,
        /// Anchored symbol name.
        symbol: String,
        /// Local link confidence in the closed interval `[0, 1]`.
        confidence: f32,
        /// Human-readable source of the inferred link.
        provenance: String,
    },
}

impl Anchor {
    /// Return the canonical repository ID containing the anchor.
    #[must_use]
    pub fn repo(&self) -> &str {
        match self {
            Self::Explicit { repo, .. } | Self::Inferred { repo, .. } => repo,
        }
    }

    /// Return the repository-relative anchor path.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Explicit { path, .. } | Self::Inferred { path, .. } => path,
        }
    }

    /// Return the anchored symbol name.
    #[must_use]
    pub fn symbol(&self) -> &str {
        match self {
            Self::Explicit { symbol, .. } | Self::Inferred { symbol, .. } => symbol,
        }
    }

    /// Return the inferred-link confidence, or `None` for an explicit anchor.
    #[must_use]
    pub const fn confidence(&self) -> Option<f32> {
        match self {
            Self::Explicit { .. } => None,
            Self::Inferred { confidence, .. } => Some(*confidence),
        }
    }
}

/// One parsed claim's frontmatter-declared state, anchors, and editable byte ranges.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteClaim {
    pub(crate) status: ClaimStatus,
    pub(crate) review: ReviewState,
    pub(crate) anchors: Vec<Anchor>,
    pub(crate) sources: BTreeMap<String, String>,
    pub(crate) status_range: Range<usize>,
    pub(crate) review_range: Range<usize>,
}

impl NoteClaim {
    /// Return the claim's current status.
    #[must_use]
    pub const fn status(&self) -> ClaimStatus {
        self.status
    }

    /// Return the claim's current review state.
    #[must_use]
    pub const fn review(&self) -> ReviewState {
        self.review
    }

    /// Return the claim's recorded anchors.
    #[must_use]
    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    /// Return the claim's per-repository source revisions.
    #[must_use]
    pub const fn sources(&self) -> &BTreeMap<String, String> {
        &self.sources
    }
}

/// Byte range of one managed region's editable interior content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRegion {
    pub(crate) claim_id: String,
    pub(crate) content: Range<usize>,
}

impl ManagedRegion {
    /// Return the claim ID this managed region belongs to.
    #[must_use]
    pub fn claim_id(&self) -> &str {
        &self.claim_id
    }

    /// Return the byte range of this region's editable interior content.
    #[must_use]
    pub fn content_range(&self) -> Range<usize> {
        self.content.clone()
    }
}
