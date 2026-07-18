//! Bounded model-free candidate generation.

use std::collections::{BTreeMap, BTreeSet};

use crate::index::{GenerationReader, LexicalHit, RetrievalEntity};
use crate::vectors::FlatVectorIndex;

use super::{OmittedCandidate, QueryEmbedding, RetrievalError, SearchRequest};

const MAX_GLOBAL_CANDIDATES: usize = 256;
const SOURCE_RESULT_LIMIT: usize = MAX_GLOBAL_CANDIDATES + 1;

#[derive(Clone, Debug)]
pub(super) struct Candidate {
    pub(super) entity_id: String,
    pub(super) repository: String,
    pub(super) revision: String,
    pub(super) path: String,
    pub(super) language: String,
    pub(super) symbol: Option<String>,
    pub(super) start_line: u32,
    pub(super) end_line: u32,
    pub(super) kind: String,
}

#[derive(Clone, Debug)]
pub(super) struct CandidatePool {
    pub(super) scorer: &'static str,
    pub(super) weight: f32,
    pub(super) candidates: Vec<Candidate>,
}

pub(super) struct CandidateGeneration {
    pub(super) pools: Vec<CandidatePool>,
    pub(super) omitted: Vec<OmittedCandidate>,
}

pub(super) fn generate(
    reader: &GenerationReader,
    request: &SearchRequest,
    query_embedding: Option<&QueryEmbedding>,
) -> Result<CandidateGeneration, RetrievalError> {
    let lexical = reader
        .lexical()
        .search_text(request.query(), SOURCE_RESULT_LIMIT)?
        .into_iter()
        .map(|hit| from_lexical(&hit))
        .collect::<Vec<_>>();
    let terms = exact_terms(request.query());
    let mut path_queries = terms.clone();
    let complete_path = request.query().trim();
    if !complete_path.is_empty() && !path_queries.iter().any(|term| term == complete_path) {
        path_queries.push(complete_path.to_owned());
    }
    let exact_path = collect_lexical(&path_queries, |term| {
        reader.lexical().exact_path(term, SOURCE_RESULT_LIMIT)
    })?;
    let exact_symbol = collect_lexical(&terms, |term| {
        reader.lexical().exact_symbol(term, SOURCE_RESULT_LIMIT)
    })?;
    let identifier_terms = terms
        .iter()
        .filter(|term| terms.len() == 1 || code_identifier(term))
        .cloned()
        .collect::<Vec<_>>();
    let exact_identifier = collect_lexical(&identifier_terms, |term| {
        reader.lexical().exact_identifier(term, SOURCE_RESULT_LIMIT)
    })?;
    let (exact_metadata, exact_metadata_omitted) = collect_graph(&terms, |term| {
        reader.graph().retrieval_exact(term, SOURCE_RESULT_LIMIT)
    })?;

    let mut pools = vec![
        pool("lexical", 1.0, lexical),
        pool("exact_path", 2.0, exact_path),
        pool("exact_symbol", 2.0, exact_symbol),
        pool("exact_identifier", 1.5, exact_identifier),
        pool("exact_metadata", 1.5, exact_metadata),
    ];
    let mut omitted = exact_metadata_omitted;
    enforce_global_bound(&pools, &omitted)?;
    let seeds = pools
        .iter()
        .flat_map(|pool| pool.candidates.iter())
        .map(|candidate| candidate.entity_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut graph = Vec::new();
    for (entity_id, resolved) in reader
        .graph()
        .retrieval_neighbors(&seeds, SOURCE_RESULT_LIMIT)?
    {
        if let Some(entity) = resolved {
            graph.push(from_graph(entity));
        } else {
            omitted.push(OmittedCandidate::new(entity_id, "stale"));
        }
    }
    pools.push(pool("graph", 0.75, graph));
    if let Some(embedding) = query_embedding {
        let (vectors, vector_omitted) = vector_candidates(reader, embedding)?;
        omitted.extend(vector_omitted);
        pools.push(pool("vectors", 1.0, vectors));
    }
    pools.retain(|pool| !pool.candidates.is_empty());
    enforce_global_bound(&pools, &omitted)?;
    Ok(CandidateGeneration { pools, omitted })
}

/// Resolve bounded cosine-similarity candidates from the pinned generation's
/// validated vector index, when one exists and matches the query embedding's
/// model fingerprint.
///
/// Any failure to open or search the optional vector index (missing file,
/// mismatched model, or corrupt data) is treated as "no vector candidates"
/// rather than a search failure, so the model-free path keeps working when
/// the index or model does not match.
fn vector_candidates(
    reader: &GenerationReader,
    embedding: &QueryEmbedding,
) -> Result<(Vec<Candidate>, Vec<OmittedCandidate>), RetrievalError> {
    let Some((vector_path, ids_path)) = reader.vectors_paths() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let Ok(index) = FlatVectorIndex::open(vector_path, ids_path, embedding.fingerprint()) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let Ok(top) = index.top_k(
        embedding.vector(),
        SOURCE_RESULT_LIMIT.min(MAX_GLOBAL_CANDIDATES),
    ) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut candidates = Vec::new();
    let mut omitted = Vec::new();
    for (entity_id, _score) in top {
        match reader.graph().retrieval_resolve(&entity_id)? {
            Some(resolved) => candidates.push(from_graph(resolved)),
            None => omitted.push(OmittedCandidate::new(entity_id, "stale")),
        }
        reject_collection_overflow(candidates.len().saturating_add(omitted.len()))?;
    }
    Ok((candidates, omitted))
}

fn enforce_global_bound(
    pools: &[CandidatePool],
    omitted: &[OmittedCandidate],
) -> Result<(), RetrievalError> {
    let identities = pools
        .iter()
        .flat_map(|pool| {
            pool.candidates
                .iter()
                .map(|candidate| candidate.entity_id.as_str())
        })
        .chain(omitted.iter().map(OmittedCandidate::entity_id))
        .collect::<BTreeSet<_>>();
    if identities.len() > MAX_GLOBAL_CANDIDATES {
        return Err(RetrievalError::CandidateBoundExceeded {
            maximum: MAX_GLOBAL_CANDIDATES,
        });
    }
    Ok(())
}

fn pool(scorer: &'static str, weight: f32, candidates: Vec<Candidate>) -> CandidatePool {
    let mut seen = BTreeSet::new();
    let mut filtered = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate.entity_id.clone()) {
            filtered.push(candidate);
        }
    }
    CandidatePool {
        scorer,
        weight,
        candidates: filtered,
    }
}

fn collect_graph<F>(
    terms: &[String],
    mut search: F,
) -> Result<(Vec<Candidate>, Vec<OmittedCandidate>), RetrievalError>
where
    F: FnMut(&str) -> Result<Vec<(String, Option<RetrievalEntity>)>, crate::index::IndexError>,
{
    let mut candidates = BTreeMap::new();
    let mut omitted = BTreeMap::new();
    for term in terms {
        for (entity_id, resolved) in search(term)? {
            if let Some(entity) = resolved {
                candidates
                    .entry(entity_id)
                    .or_insert_with(|| from_graph(entity));
            } else {
                omitted
                    .entry(entity_id.clone())
                    .or_insert_with(|| OmittedCandidate::new(entity_id, "stale"));
            }
            reject_collection_overflow(candidates.len().saturating_add(omitted.len()))?;
        }
    }
    Ok((
        candidates.into_values().collect(),
        omitted.into_values().collect(),
    ))
}

fn collect_lexical<F>(terms: &[String], mut search: F) -> Result<Vec<Candidate>, RetrievalError>
where
    F: FnMut(&str) -> Result<Vec<LexicalHit>, crate::index::IndexError>,
{
    let mut candidates = BTreeMap::new();
    for term in terms {
        for hit in search(term)? {
            candidates
                .entry(hit.entity_id().to_owned())
                .or_insert_with(|| from_lexical(&hit));
            reject_collection_overflow(candidates.len())?;
        }
    }
    Ok(candidates.into_values().collect())
}

const fn reject_collection_overflow(length: usize) -> Result<(), RetrievalError> {
    if length > MAX_GLOBAL_CANDIDATES {
        return Err(RetrievalError::CandidateBoundExceeded {
            maximum: MAX_GLOBAL_CANDIDATES,
        });
    }
    Ok(())
}

fn exact_terms(query: &str) -> Vec<String> {
    let mut terms = BTreeSet::new();
    for raw in query.split_whitespace() {
        let term = raw.trim_matches(|character: char| {
            !character.is_alphanumeric() && !matches!(character, '_' | '-' | '/' | '.' | ':' | '#')
        });
        if !term.is_empty() && term.len() <= 1_024 {
            terms.insert(term.to_owned());
        }
    }
    terms.into_iter().collect()
}

fn code_identifier(term: &str) -> bool {
    term.contains(['_', ':', '#']) || term.bytes().skip(1).any(|byte| byte.is_ascii_uppercase())
}

fn from_lexical(hit: &LexicalHit) -> Candidate {
    Candidate {
        entity_id: hit.entity_id().to_owned(),
        repository: hit.repository().to_owned(),
        revision: hit.revision().to_owned(),
        path: hit.path().to_owned(),
        language: hit.language().to_owned(),
        symbol: hit.symbol().map(str::to_owned),
        start_line: hit.start_line(),
        end_line: hit.end_line(),
        kind: hit.kind().to_owned(),
    }
}

fn from_graph(entity: RetrievalEntity) -> Candidate {
    Candidate {
        entity_id: entity.entity_id,
        repository: entity.repository,
        revision: entity.revision,
        path: entity.path,
        language: entity.language,
        symbol: entity.symbol,
        start_line: entity.start_line,
        end_line: entity.end_line,
        kind: entity.kind,
    }
}
