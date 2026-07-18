//! Pure claim state transitions from caller-supplied anchor impact analysis.
//!
//! This module has no dependency on the ingestion or index modules. A caller
//! wires a real evidence-graph diff into [`ImpactInput::affected`] during
//! integration; here the transition rules are expressed only in terms of a
//! claim's recorded anchors and a map of which anchor targets changed.

use std::collections::HashMap;

use super::note::{Anchor, ClaimStatus};

/// Resolution of one claim anchor's target against a caller-computed code diff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolution {
    /// The anchor's exact target changed in this diff.
    DirectlyAffected,
    /// The anchor's target did not change directly, but is reachable from
    /// changed code through the evidence graph's neighborhood.
    TransitivelyImpacted,
    /// An anchor's declared target no longer resolves in the current index.
    Unresolvable,
}

/// Stable lookup key for one anchor's code target.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AnchorKey {
    /// Canonical repository ID.
    pub repo: String,
    /// Repository-relative anchor path.
    pub path: String,
    /// Anchored symbol name.
    pub symbol: String,
}

impl AnchorKey {
    /// Build the lookup key for one claim anchor.
    #[must_use]
    pub fn from_anchor(anchor: &Anchor) -> Self {
        Self {
            repo: anchor.repo().to_owned(),
            path: anchor.path().to_owned(),
            symbol: anchor.symbol().to_owned(),
        }
    }
}

/// Decoupled input to the pure claim state-transition function.
#[derive(Clone, Debug, Default)]
pub struct ImpactInput {
    /// The claim's status before this impact analysis.
    pub current_status: Option<ClaimStatus>,
    /// The claim's currently recorded anchors.
    pub anchors: Vec<Anchor>,
    /// Resolution of anchor targets touched by the current code change.
    pub affected: HashMap<AnchorKey, Resolution>,
}

/// Compute the next [`ClaimStatus`] for one claim from its anchor impact analysis.
///
/// Implements the state-transition table from the knowledge-compiler design:
///
/// - a directly affected explicit anchor, or a directly affected inferred
///   anchor at confidence `0.85` or higher, becomes [`ClaimStatus::PendingUpdate`]
/// - a directly affected inferred anchor at confidence `0.60` up to `0.85`, or
///   any anchor that is only transitively impacted at confidence `0.60` or
///   higher (or is explicit), becomes [`ClaimStatus::Stale`]
/// - an inferred anchor below confidence `0.60` is unaffected regardless of its
///   recorded resolution; it remains a search hint and is not a dependency
/// - an unresolvable anchor becomes [`ClaimStatus::Broken`] when explicit
/// - a claim with no affected anchors retains `current_status` (defaulting to
///   [`ClaimStatus::Current`] when the caller has no prior status)
///
/// Explicit anchors always outrank inferred anchors because `Broken` and
/// `PendingUpdate` are the highest-priority outcomes across every anchor.
#[must_use]
pub fn transition(input: &ImpactInput) -> ClaimStatus {
    let mut broken = false;
    let mut pending = false;
    let mut stale = false;

    for anchor in &input.anchors {
        let key = AnchorKey::from_anchor(anchor);
        let Some(&resolution) = input.affected.get(&key) else {
            continue;
        };
        let confidence = anchor.confidence();

        match resolution {
            Resolution::Unresolvable => {
                if confidence.is_none() {
                    broken = true;
                }
            }
            Resolution::DirectlyAffected => {
                if let Some(confidence) = confidence {
                    if confidence >= 0.85 {
                        pending = true;
                    } else if confidence >= 0.60 {
                        stale = true;
                    }
                } else {
                    pending = true;
                }
            }
            Resolution::TransitivelyImpacted => {
                if let Some(confidence) = confidence {
                    if confidence >= 0.60 {
                        stale = true;
                    }
                } else {
                    stale = true;
                }
            }
        }
    }

    if broken {
        ClaimStatus::Broken
    } else if pending {
        ClaimStatus::PendingUpdate
    } else if stale {
        ClaimStatus::Stale
    } else {
        input.current_status.unwrap_or(ClaimStatus::Current)
    }
}
