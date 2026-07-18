//! Living-note Markdown parsing and byte-exact managed-region editing.
//!
//! A note is ordinary Markdown with a YAML frontmatter block and zero or more
//! `<!-- pebble:managed claim=ID -->...<!-- /pebble:managed -->` regions. Parsing
//! locates the exact byte ranges Pebble is allowed to rewrite (a claim's `status`
//! and `review` frontmatter scalars, plus its managed region's interior bytes) so
//! that every other byte of the original file can be preserved exactly on edit.

mod scan;
mod types;

use std::collections::BTreeMap;
use std::ops::Range;

use serde::Serialize;

pub use types::{Anchor, ClaimStatus, ManagedRegion, NoteClaim, ReviewState};

use crate::knowledge::KnowledgeError;

#[derive(Serialize)]
struct ClaimSnapshot<'a> {
    status: &'a str,
    review: &'a str,
    body: &'a str,
}

/// A parsed living-knowledge note.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    raw: String,
    schema: i64,
    id: String,
    title: String,
    claims: BTreeMap<String, NoteClaim>,
    regions: BTreeMap<String, ManagedRegion>,
}

impl Note {
    /// Parse a note from its full Markdown text.
    ///
    /// # Errors
    ///
    /// Returns an error when the frontmatter block is missing, unclosed, or
    /// malformed, when a claim's status or review token is unrecognized, or
    /// when managed-region markers are malformed, unmatched, or inconsistent
    /// with the declared claims.
    pub fn parse(raw: &str) -> Result<Self, KnowledgeError> {
        let (frontmatter, body_start) = scan::split_frontmatter(raw)?;
        let frontmatter_text = &raw[frontmatter.clone()];
        let document = scan::load_frontmatter(frontmatter_text)?;

        let schema = document["pebble_schema"].as_i64().ok_or_else(|| {
            KnowledgeError::MalformedFrontmatter("pebble_schema must be an integer".to_owned())
        })?;
        let id = scan::document_field(&document, "pebble_id")?;
        let title = document["title"].as_str().unwrap_or_default().to_owned();

        let ranges = scan::scan_claim_ranges(frontmatter_text, frontmatter.start);
        let mut claims = BTreeMap::new();
        if let Some(hash) = document["pebble_claims"].as_hash() {
            for (key, value) in hash {
                let claim_id = key
                    .as_str()
                    .ok_or_else(|| {
                        KnowledgeError::MalformedFrontmatter("claim ID must be a string".to_owned())
                    })?
                    .to_owned();
                let claim = scan::build_claim(value, ranges.get(&claim_id))?;
                claims.insert(claim_id, claim);
            }
        }

        let regions = scan::scan_managed_regions(raw, body_start, &claims)?;
        for claim_id in claims.keys() {
            if !regions.contains_key(claim_id) {
                return Err(KnowledgeError::MissingManagedRegion(claim_id.clone()));
            }
        }

        Ok(Self {
            raw: raw.to_owned(),
            schema,
            id,
            title,
            claims,
            regions,
        })
    }

    /// Return the note's original full text.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Return the note's declared frontmatter schema version.
    #[must_use]
    pub const fn schema(&self) -> i64 {
        self.schema
    }

    /// Return the note's stable ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Return the note's human-readable title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Return one claim by ID.
    #[must_use]
    pub fn claim(&self, claim_id: &str) -> Option<&NoteClaim> {
        self.claims.get(claim_id)
    }

    /// Return every claim declared in this note's frontmatter.
    #[must_use]
    pub const fn claims(&self) -> &BTreeMap<String, NoteClaim> {
        &self.claims
    }

    /// Return one managed region by claim ID.
    #[must_use]
    pub fn managed_region(&self, claim_id: &str) -> Option<&ManagedRegion> {
        self.regions.get(claim_id)
    }

    /// Return one claim's current managed-region body text.
    #[must_use]
    pub fn managed_body(&self, claim_id: &str) -> Option<&str> {
        let region = self.regions.get(claim_id)?;
        Some(&self.raw[region.content_range()])
    }

    /// Serialize a compact snapshot of one claim's status, review, and body.
    ///
    /// # Errors
    ///
    /// Returns an error when the claim does not exist in this note.
    pub fn claim_snapshot(&self, claim_id: &str) -> Result<String, KnowledgeError> {
        let claim = self
            .claims
            .get(claim_id)
            .ok_or_else(|| KnowledgeError::ClaimNotFound(claim_id.to_owned()))?;
        let body = self
            .managed_body(claim_id)
            .ok_or_else(|| KnowledgeError::ClaimNotFound(claim_id.to_owned()))?;
        let snapshot = ClaimSnapshot {
            status: claim.status().as_str(),
            review: claim.review().as_str(),
            body,
        };
        Ok(serde_json::to_string(&snapshot)?)
    }

    /// Build the new full note text after replacing one claim's managed body
    /// and Pebble-owned status and review frontmatter scalars.
    ///
    /// Every byte outside the named managed region and the claim's `status`
    /// and `review` scalar values is copied unchanged from the original note.
    ///
    /// # Errors
    ///
    /// Returns an error when the claim or its managed region does not exist,
    /// or when the computed edit ranges overlap.
    pub fn apply_claim_patch(
        &self,
        claim_id: &str,
        new_body: &str,
        new_status: ClaimStatus,
        new_review: Option<ReviewState>,
    ) -> Result<String, KnowledgeError> {
        let claim = self
            .claims
            .get(claim_id)
            .ok_or_else(|| KnowledgeError::ClaimNotFound(claim_id.to_owned()))?;
        let region = self
            .regions
            .get(claim_id)
            .ok_or_else(|| KnowledgeError::ClaimNotFound(claim_id.to_owned()))?;

        let mut edits: Vec<(Range<usize>, String)> =
            vec![(region.content_range(), new_body.to_owned())];
        edits.push((claim.status_range.clone(), new_status.as_str().to_owned()));
        if let Some(review) = new_review {
            edits.push((claim.review_range.clone(), review.as_str().to_owned()));
        }
        edits.sort_by_key(|(range, _)| range.start);
        for pair in edits.windows(2) {
            if pair[0].0.end > pair[1].0.start {
                return Err(KnowledgeError::OverlappingEdit);
            }
        }

        let mut result = String::with_capacity(self.raw.len());
        let mut cursor = 0usize;
        for (range, replacement) in &edits {
            result.push_str(&self.raw[cursor..range.start]);
            result.push_str(replacement);
            cursor = range.end;
        }
        result.push_str(&self.raw[cursor..]);
        Ok(result)
    }
}
