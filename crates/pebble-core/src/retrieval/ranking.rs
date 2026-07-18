//! Deterministic active-scorer reciprocal-rank fusion.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::domain::ScoreExplanation;

use super::candidate::{Candidate, CandidatePool};

const RRF_OFFSET: f32 = 60.0;

#[derive(Clone, Debug)]
pub(super) struct RankedCandidate {
    pub(super) candidate: Candidate,
    pub(super) score: f32,
    pub(super) explanations: Vec<ScoreExplanation>,
}

pub(super) fn fuse(pools: &[CandidatePool]) -> Vec<RankedCandidate> {
    let total_weight = pools.iter().map(|pool| pool.weight).sum::<f32>();
    if total_weight <= f32::EPSILON {
        return Vec::new();
    }
    let mut combined: BTreeMap<String, RankedCandidate> = BTreeMap::new();
    for pool in pools {
        let normalized_weight = pool.weight / total_weight;
        for (index, candidate) in pool.candidates.iter().enumerate() {
            let rank = u16::try_from(index.saturating_add(1))
                .map_or_else(|_| f32::from(u16::MAX), f32::from);
            let contribution = normalized_weight / (RRF_OFFSET + rank);
            let entry = combined
                .entry(candidate.entity_id.clone())
                .or_insert_with(|| RankedCandidate {
                    candidate: candidate.clone(),
                    score: 0.0,
                    explanations: Vec::new(),
                });
            entry.score += contribution;
            entry.explanations.push(ScoreExplanation {
                scorer: pool.scorer.to_owned(),
                score: contribution,
                explanation: format!("w={normalized_weight:.3},r={}", index.saturating_add(1)),
            });
        }
    }
    let mut ranked = combined.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.candidate.path.cmp(&right.candidate.path))
            .then_with(|| left.candidate.symbol.cmp(&right.candidate.symbol))
            .then_with(|| left.candidate.entity_id.cmp(&right.candidate.entity_id))
    });
    ranked
}
