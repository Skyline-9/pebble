//! Byte-exact frontmatter and managed-region scanning.
//!
//! These scanners locate the exact byte ranges of a claim's `status` and
//! `review` frontmatter scalars and of each managed region's interior content,
//! so the rest of the note can be preserved byte-for-byte on edit.

use std::collections::BTreeMap;
use std::ops::Range;

use yaml_rust2::{Yaml, YamlLoader};

use super::types::{Anchor, ClaimStatus, ManagedRegion, NoteClaim, ReviewState};
use crate::knowledge::KnowledgeError;

pub(super) fn load_frontmatter(text: &str) -> Result<Yaml, KnowledgeError> {
    YamlLoader::load_from_str(text)
        .map_err(|error| KnowledgeError::MalformedFrontmatter(error.to_string()))?
        .into_iter()
        .next()
        .ok_or_else(|| KnowledgeError::MalformedFrontmatter("frontmatter is empty".to_owned()))
}

fn yaml_str(node: &Yaml, key: &str) -> Result<String, KnowledgeError> {
    node[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| KnowledgeError::MalformedFrontmatter(format!("'{key}' must be a string")))
}

fn yaml_confidence(node: &Yaml) -> Result<f32, KnowledgeError> {
    let text = match node {
        Yaml::Real(value) => value.clone(),
        Yaml::Integer(value) => value.to_string(),
        _ => {
            return Err(KnowledgeError::MalformedFrontmatter(
                "anchor confidence must be numeric".to_owned(),
            ));
        }
    };
    text.parse::<f32>().map_err(|_| {
        KnowledgeError::MalformedFrontmatter("anchor confidence must be a valid float".to_owned())
    })
}

fn parse_anchors(node: &Yaml) -> Result<Vec<Anchor>, KnowledgeError> {
    let mut anchors = Vec::new();
    if let Some(explicit) = node["explicit"].as_vec() {
        for item in explicit {
            anchors.push(Anchor::Explicit {
                repo: yaml_str(item, "repo")?,
                path: yaml_str(item, "path")?,
                symbol: yaml_str(item, "symbol")?,
            });
        }
    }
    if let Some(inferred) = node["inferred"].as_vec() {
        for item in inferred {
            anchors.push(Anchor::Inferred {
                repo: yaml_str(item, "repo")?,
                path: yaml_str(item, "path")?,
                symbol: yaml_str(item, "symbol")?,
                confidence: yaml_confidence(&item["confidence"])?,
                provenance: yaml_str(item, "provenance")?,
            });
        }
    }
    Ok(anchors)
}

pub(super) fn document_field(document: &Yaml, key: &str) -> Result<String, KnowledgeError> {
    yaml_str(document, key)
}

pub(super) fn build_claim(
    value: &Yaml,
    ranges: Option<&ClaimRanges>,
) -> Result<NoteClaim, KnowledgeError> {
    let status = ClaimStatus::try_from(yaml_str(value, "status")?.as_str())?;
    let review = ReviewState::try_from(yaml_str(value, "review")?.as_str())?;
    let anchors = parse_anchors(&value["anchors"])?;
    let mut sources = BTreeMap::new();
    if let Some(hash) = value["sources"].as_hash() {
        for (repo, entry) in hash {
            let repo = repo.as_str().ok_or_else(|| {
                KnowledgeError::MalformedFrontmatter("source repo must be a string".to_owned())
            })?;
            sources.insert(repo.to_owned(), yaml_str(entry, "revision")?);
        }
    }
    let ranges = ranges.ok_or_else(|| {
        KnowledgeError::MalformedFrontmatter("claim is missing status or review".to_owned())
    })?;
    let status_range = ranges.status.clone().ok_or_else(|| {
        KnowledgeError::MalformedFrontmatter("claim is missing a status scalar".to_owned())
    })?;
    let review_range = ranges.review.clone().ok_or_else(|| {
        KnowledgeError::MalformedFrontmatter("claim is missing a review scalar".to_owned())
    })?;
    Ok(NoteClaim {
        status,
        review,
        anchors,
        sources,
        status_range,
        review_range,
    })
}

pub(super) fn split_frontmatter(raw: &str) -> Result<(Range<usize>, usize), KnowledgeError> {
    let mut lines = raw.split_inclusive('\n');
    let first = lines.next().ok_or(KnowledgeError::MissingFrontmatter)?;
    if first.trim_end_matches(['\n', '\r']) != "---" {
        return Err(KnowledgeError::MissingFrontmatter);
    }
    let mut offset = first.len();
    let content_start = offset;
    for line in lines {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            return Ok((content_start..offset, offset + line.len()));
        }
        offset += line.len();
    }
    Err(KnowledgeError::UnclosedFrontmatter)
}

#[derive(Default)]
pub(super) struct ClaimRanges {
    status: Option<Range<usize>>,
    review: Option<Range<usize>>,
}

fn offset_in(text: &str, slice: &str) -> usize {
    slice.as_ptr() as usize - text.as_ptr() as usize
}

fn value_range(text: &str, base: usize, line: &str, prefix: &str) -> Option<Range<usize>> {
    let content = line.trim_end_matches(['\n', '\r']);
    let value = content.trim_start().strip_prefix(prefix)?.trim();
    if value.is_empty() {
        return None;
    }
    let start = base + offset_in(text, value);
    Some(start..start + value.len())
}

pub(super) fn scan_claim_ranges(text: &str, base: usize) -> BTreeMap<String, ClaimRanges> {
    let mut result: BTreeMap<String, ClaimRanges> = BTreeMap::new();
    let mut in_claims = false;
    let mut claims_block_indent = 0usize;
    let mut claim_indent: Option<usize> = None;
    let mut current_claim: Option<String> = None;

    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        if content.trim().is_empty() {
            continue;
        }
        let trimmed = content.trim_start();
        let indent = content.len() - trimmed.len();

        if !in_claims {
            if indent == 0 && trimmed == "pebble_claims:" {
                in_claims = true;
                claims_block_indent = indent;
            }
            continue;
        }
        if indent <= claims_block_indent {
            in_claims = false;
            current_claim = None;
            claim_indent = None;
            continue;
        }
        let level = *claim_indent.get_or_insert(indent);
        if indent == level {
            current_claim = trimmed.strip_suffix(':').map(str::to_owned);
            continue;
        }
        let Some(claim_id) = current_claim.clone() else {
            continue;
        };
        if let Some(range) = value_range(text, base, line, "status:") {
            result.entry(claim_id).or_default().status = Some(range);
        } else if let Some(range) = value_range(text, base, line, "review:") {
            result.entry(claim_id).or_default().review = Some(range);
        }
    }
    result
}

pub(super) fn scan_managed_regions(
    raw: &str,
    body_start: usize,
    claims: &BTreeMap<String, NoteClaim>,
) -> Result<BTreeMap<String, ManagedRegion>, KnowledgeError> {
    let body = &raw[body_start..];
    let mut regions = BTreeMap::new();
    let mut open: Option<(String, usize)> = None;

    for line in body.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<!-- pebble:managed claim=") {
            let id = rest
                .strip_suffix("-->")
                .map(str::trim)
                .ok_or(KnowledgeError::MalformedManagedMarker)?;
            if open.is_some() {
                return Err(KnowledgeError::MalformedManagedMarker);
            }
            if !claims.contains_key(id) {
                return Err(KnowledgeError::UnknownClaim(id.to_owned()));
            }
            let content_start = body_start + offset_in(body, line) + line.len();
            open = Some((id.to_owned(), content_start));
            continue;
        }
        if trimmed == "<!-- /pebble:managed -->" {
            let (id, content_start) = open.take().ok_or(KnowledgeError::MalformedManagedMarker)?;
            let content_end = body_start + offset_in(body, line);
            if regions.contains_key(&id) {
                return Err(KnowledgeError::DuplicateManagedRegion(id));
            }
            regions.insert(
                id.clone(),
                ManagedRegion {
                    claim_id: id,
                    content: content_start..content_end,
                },
            );
        }
    }
    if let Some((id, _)) = open {
        return Err(KnowledgeError::UnclosedManagedRegion(id));
    }
    Ok(regions)
}
