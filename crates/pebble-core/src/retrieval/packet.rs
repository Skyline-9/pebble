//! Citation resolution, diversity, truncation, and packet budgeting.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    Citation, EvidenceDiagnostic, EvidenceItem, EvidencePacket, RepositoryId, WorktreeRevision,
};
use crate::error::DomainError;
use crate::index::GenerationReader;

use super::ranking::RankedCandidate;
use super::trace::{OmittedCandidate, TraceCandidate};
use super::{RetrievalError, SearchRequest};

pub(super) struct PacketBuild {
    pub(super) packet: EvidencePacket,
    pub(super) estimated_tokens: u32,
    pub(super) selected: Vec<TraceCandidate>,
    pub(super) omitted: Vec<OmittedCandidate>,
}

#[allow(clippy::too_many_lines)]
pub(super) fn build(
    reader: &GenerationReader,
    request: &SearchRequest,
    ranked: &[RankedCandidate],
) -> Result<PacketBuild, RetrievalError> {
    let mut omitted = Vec::new();
    let mut resolvable = Vec::new();
    for ranked_candidate in ranked {
        let Some(resolved) = reader
            .graph()
            .retrieval_resolve(&ranked_candidate.candidate.entity_id)?
        else {
            omitted.push(OmittedCandidate::new(
                ranked_candidate.candidate.entity_id.clone(),
                "stale",
            ));
            continue;
        };
        if resolved.repository != ranked_candidate.candidate.repository
            || resolved.revision != ranked_candidate.candidate.revision
            || resolved.path != ranked_candidate.candidate.path
            || resolved.language != ranked_candidate.candidate.language
            || resolved.kind != ranked_candidate.candidate.kind
            || resolved.symbol != ranked_candidate.candidate.symbol
            || resolved.start_line != ranked_candidate.candidate.start_line
            || resolved.end_line != ranked_candidate.candidate.end_line
        {
            omitted.push(OmittedCandidate::new(
                ranked_candidate.candidate.entity_id.clone(),
                "metadata_disagreement",
            ));
            continue;
        }
        if !request.matches_resolved(&resolved) {
            omitted.push(OmittedCandidate::new(
                ranked_candidate.candidate.entity_id.clone(),
                "metadata_filter",
            ));
            continue;
        }
        let Some(content) = resolved.content.as_deref() else {
            omitted.push(OmittedCandidate::new(
                ranked_candidate.candidate.entity_id.clone(),
                "unresolvable",
            ));
            continue;
        };
        if content.trim().is_empty() {
            omitted.push(OmittedCandidate::new(
                ranked_candidate.candidate.entity_id.clone(),
                "empty",
            ));
            continue;
        }
        resolvable.push((ranked_candidate, resolved));
    }
    let ordered = diversify(&resolvable);
    let mut items = Vec::new();
    let mut selected = Vec::new();
    let selected_indices = ordered
        .into_iter()
        .take(request.max_results())
        .collect::<BTreeSet<_>>();
    for (index, (ranked_candidate, _)) in resolvable.iter().enumerate() {
        if !selected_indices.contains(&index) {
            omitted.push(OmittedCandidate::new(
                ranked_candidate.candidate.entity_id.clone(),
                "result_limit",
            ));
        }
    }
    for index in selected_indices {
        let (ranked_candidate, resolved) = &resolvable[index];
        let content = resolved.content.as_deref().ok_or_else(|| {
            RetrievalError::InvalidRequest("resolved content vanished".to_owned())
        })?;
        let excerpt = truncate(content, request.budget_tokens());
        let repository = RepositoryId::try_from(resolved.repository.clone())?;
        let revision = parse_revision(&resolved.revision)?;
        let citation = Citation::new(
            repository,
            revision,
            resolved.path.clone(),
            resolved.start_line,
            excerpt_end_line(resolved.start_line, resolved.end_line, &excerpt),
        )?;
        items.push(EvidenceItem {
            citation: citation.clone(),
            content: excerpt,
            score_explanations: ranked_candidate.explanations.clone(),
        });
        selected.push(TraceCandidate::new(
            ranked_candidate.candidate.entity_id.clone(),
            ranked_candidate.score,
            citation,
        ));
    }

    let (packet, estimated_tokens) = fit_packet(
        request.budget_tokens(),
        &mut items,
        &mut selected,
        &mut omitted,
    )?;
    Ok(PacketBuild {
        packet,
        estimated_tokens,
        selected,
        omitted,
    })
}

fn diversify(ranked: &[(&RankedCandidate, crate::index::RetrievalEntity)]) -> Vec<usize> {
    let mut selected = Vec::new();
    let mut paths = BTreeMap::<&str, usize>::new();
    let mut symbols = BTreeMap::<&str, usize>::new();
    for (index, (_, resolved)) in ranked.iter().enumerate() {
        if paths.len() == 3 {
            break;
        }
        if paths.contains_key(resolved.path.as_str()) {
            continue;
        }
        paths.insert(resolved.path.as_str(), 1);
        if let Some(symbol) = resolved.symbol.as_deref() {
            *symbols.entry(symbol).or_default() += 1;
        }
        selected.push(index);
    }
    for (index, (_, resolved)) in ranked.iter().enumerate() {
        if selected.contains(&index) {
            continue;
        }
        let path_count = paths
            .get(resolved.path.as_str())
            .copied()
            .unwrap_or_default();
        let symbol_count = resolved
            .symbol
            .as_deref()
            .and_then(|symbol| symbols.get(symbol).copied())
            .unwrap_or_default();
        if path_count >= 2 || symbol_count >= 2 {
            continue;
        }
        *paths.entry(resolved.path.as_str()).or_default() += 1;
        if let Some(symbol) = resolved.symbol.as_deref() {
            *symbols.entry(symbol).or_default() += 1;
        }
        selected.push(index);
    }
    for index in 0..ranked.len() {
        if !selected.contains(&index) {
            selected.push(index);
        }
    }
    selected
}

fn truncate(content: &str, budget_tokens: u32) -> String {
    let maximum_bytes = usize::try_from(budget_tokens).unwrap_or(usize::MAX);
    if content.len() <= maximum_bytes {
        return content.to_owned();
    }
    let mut byte_limit = maximum_bytes.min(content.len());
    while !content.is_char_boundary(byte_limit) {
        byte_limit = byte_limit.saturating_sub(1);
    }
    let prefix = &content[..byte_limit];
    let boundary = prefix
        .rfind("\n\n")
        .map(|index| index.saturating_add(2))
        .or_else(|| prefix.rfind('\n').map(|index| index.saturating_add(1)))
        .or_else(|| {
            prefix
                .rfind([';', '}', '{', '.'])
                .map(|index| index.saturating_add(1))
        })
        .unwrap_or(byte_limit);
    prefix[..boundary].trim_end().to_owned()
}

fn fit_packet(
    budget_tokens: u32,
    items: &mut Vec<EvidenceItem>,
    selected: &mut Vec<TraceCandidate>,
    omitted: &mut Vec<OmittedCandidate>,
) -> Result<(EvidencePacket, u32), RetrievalError> {
    loop {
        let packet = EvidencePacket::new(budget_tokens, items.clone(), diagnostics(items))?;
        let packet_bytes = serde_json::to_vec(&packet)?.len();
        if packet_bytes <= usize::try_from(budget_tokens).unwrap_or(usize::MAX) {
            return Ok((packet, u32::try_from(packet_bytes).unwrap_or(u32::MAX)));
        }
        let Some(index) = budget_trim_index(items) else {
            return Err(RetrievalError::InvalidRequest(
                "evidence packet structure exceeds its minimum budget".to_owned(),
            ));
        };
        let item = &items[index];
        let excess =
            packet_bytes.saturating_sub(usize::try_from(budget_tokens).unwrap_or(usize::MAX));
        let target = item.content.len().saturating_sub(excess.max(1));
        let shortened = truncate(&item.content, u32::try_from(target).unwrap_or(u32::MAX));
        if shortened.is_empty() || shortened.len() >= item.content.len() {
            let omitted_candidate = selected.remove(index);
            omitted.push(OmittedCandidate::new(
                omitted_candidate.entity_id().to_owned(),
                "budget",
            ));
            items.remove(index);
            continue;
        }
        let repository = item.citation.repository().clone();
        let revision = item.citation.revision().clone();
        let path = item.citation.path().to_owned();
        let start = item.citation.start_line();
        let end = excerpt_end_line(start, item.citation.end_line(), &shortened);
        let citation = Citation::new(repository, revision, path, start, end)?;
        items[index].content = shortened;
        items[index].citation = citation.clone();
        selected[index] = TraceCandidate::new(
            selected[index].entity_id().to_owned(),
            selected[index].score(),
            citation,
        );
    }
}

fn budget_trim_index(items: &[EvidenceItem]) -> Option<usize> {
    let mut paths = BTreeMap::new();
    for item in items {
        *paths.entry(item.citation.path()).or_insert(0_usize) += 1;
    }
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| paths[item.citation.path()] > 1)
        .max_by_key(|(_, item)| item.content.len())
        .or_else(|| {
            items
                .iter()
                .enumerate()
                .max_by_key(|(_, item)| item.content.len())
        })
        .map(|(index, _)| index)
}

fn diagnostics(items: &[EvidenceItem]) -> Vec<EvidenceDiagnostic> {
    let mut diagnostics = Vec::new();
    if super::contradiction::detect(items.iter().map(|item| item.content.as_str())) {
        diagnostics.push(EvidenceDiagnostic {
            code: "contradictory_evidence".to_owned(),
            message: "Selected sources contain opposing assertions; no answer was synthesized."
                .to_owned(),
        });
    }
    if items.is_empty() {
        diagnostics.push(EvidenceDiagnostic {
            code: "no_resolvable_evidence".to_owned(),
            message: "No current candidate could be emitted within the evidence budget.".to_owned(),
        });
    }
    diagnostics
}

fn excerpt_end_line(start: u32, maximum: u32, excerpt: &str) -> u32 {
    let lines = u32::try_from(excerpt.lines().count()).unwrap_or(u32::MAX);
    start.saturating_add(lines.saturating_sub(1)).min(maximum)
}

fn parse_revision(value: &str) -> Result<WorktreeRevision, DomainError> {
    value.split_once("+dirty.").map_or_else(
        || WorktreeRevision::clean(value),
        |(base, dirty)| WorktreeRevision::dirty(base, dirty),
    )
}
