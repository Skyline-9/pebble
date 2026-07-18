#![forbid(unsafe_code)]

//! Living-note parser and claim state-transition unit tests.

use std::collections::HashMap;

use pebble_core::knowledge::{
    Anchor, AnchorKey, ClaimStatus, ImpactInput, KnowledgeError, Note, Resolution, ReviewState,
    transition,
};

const NOTE: &str = r"---
pebble_schema: 1
pebble_id: note_authentication
title: Authentication
custom_field: keep-me
pebble_claims:
  session-validation:
    status: current
    review: agent_generated
    anchors:
      explicit:
        - repo: pebble
          path: src/auth/session.rs
          symbol: validate_session
    sources:
      pebble:
        revision: 2f0c38a
  token-refresh:
    status: current
    review: agent_generated
    anchors:
      inferred:
        - repo: pebble
          path: src/auth/token.rs
          symbol: refresh_token
          confidence: 0.9
          provenance: embedding-similarity
    sources:
      pebble:
        revision: 2f0c38a
---

## Context

This paragraph is owned by the author.

<!-- pebble:managed claim=session-validation -->
## Session validation

`validate_session` checks expiry before loading the user.

Sources:
- `pebble@2f0c38a:src/auth/session.rs#validate_session:L42-L68`
<!-- /pebble:managed -->

## More human prose here, untouched.

<!-- pebble:managed claim=token-refresh -->
## Token refresh

`refresh_token` rotates the session token.
<!-- /pebble:managed -->
";

#[test]
fn parses_frontmatter_and_claims() -> Result<(), Box<dyn std::error::Error>> {
    let note = Note::parse(NOTE)?;
    assert_eq!(note.schema(), 1);
    assert_eq!(note.id(), "note_authentication");
    assert_eq!(note.title(), "Authentication");
    assert_eq!(note.claims().len(), 2);

    let session = note.claim("session-validation").ok_or("missing claim")?;
    assert_eq!(session.status(), ClaimStatus::Current);
    assert_eq!(session.review(), ReviewState::AgentGenerated);
    assert_eq!(session.anchors().len(), 1);
    assert!(matches!(session.anchors()[0], Anchor::Explicit { .. }));
    assert_eq!(
        session.sources().get("pebble").map(String::as_str),
        Some("2f0c38a")
    );

    let refresh = note.claim("token-refresh").ok_or("missing claim")?;
    let Anchor::Inferred {
        confidence,
        provenance,
        ..
    } = &refresh.anchors()[0]
    else {
        return Err("expected an inferred anchor".into());
    };
    assert!((*confidence - 0.9).abs() < f32::EPSILON);
    assert_eq!(provenance, "embedding-similarity");
    Ok(())
}

#[test]
fn managed_body_contains_expected_prose() -> Result<(), Box<dyn std::error::Error>> {
    let note = Note::parse(NOTE)?;
    let body = note
        .managed_body("session-validation")
        .ok_or("missing region")?;
    assert!(body.contains("checks expiry"));
    assert!(!body.contains("pebble:managed"));
    Ok(())
}

#[test]
fn no_op_patch_reproduces_original_bytes_exactly() -> Result<(), Box<dyn std::error::Error>> {
    let note = Note::parse(NOTE)?;
    let body = note
        .managed_body("session-validation")
        .ok_or("missing region")?
        .to_owned();
    let rewritten = note.apply_claim_patch(
        "session-validation",
        &body,
        ClaimStatus::Current,
        Some(ReviewState::AgentGenerated),
    )?;
    assert_eq!(rewritten, NOTE);
    Ok(())
}

#[test]
fn patch_touches_only_its_managed_region_and_claim_scalars()
-> Result<(), Box<dyn std::error::Error>> {
    let note = Note::parse(NOTE)?;
    let rewritten = note.apply_claim_patch(
        "session-validation",
        "## Session validation\n\nRewritten prose.\n",
        ClaimStatus::PendingUpdate,
        None,
    )?;

    // Non-Pebble frontmatter, the other claim, and unrelated human prose survive unchanged.
    assert!(rewritten.contains("custom_field: keep-me"));
    assert!(rewritten.contains("This paragraph is owned by the author."));
    assert!(rewritten.contains("## More human prose here, untouched."));
    assert!(rewritten.contains("`refresh_token` rotates the session token."));

    // The edited claim's status changed; review was left untouched.
    let edited = Note::parse(&rewritten)?;
    assert_eq!(
        edited
            .claim("session-validation")
            .ok_or("missing claim")?
            .status(),
        ClaimStatus::PendingUpdate
    );
    assert_eq!(
        edited
            .claim("session-validation")
            .ok_or("missing claim")?
            .review(),
        ReviewState::AgentGenerated
    );
    assert_eq!(
        edited
            .claim("token-refresh")
            .ok_or("missing claim")?
            .status(),
        ClaimStatus::Current
    );
    assert_eq!(
        edited
            .managed_body("session-validation")
            .ok_or("missing region")?,
        "## Session validation\n\nRewritten prose.\n"
    );
    Ok(())
}

#[test]
fn rejects_missing_and_unclosed_frontmatter() {
    assert!(matches!(
        Note::parse("no frontmatter here"),
        Err(KnowledgeError::MissingFrontmatter)
    ));
    assert!(matches!(
        Note::parse("---\npebble_schema: 1\n"),
        Err(KnowledgeError::UnclosedFrontmatter)
    ));
}

#[test]
fn rejects_unrecognized_status_and_review_tokens() {
    assert!(
        matches!(ClaimStatus::try_from("mystery"), Err(KnowledgeError::InvalidClaimStatus(value)) if value == "mystery")
    );
    assert!(
        matches!(ReviewState::try_from("mystery"), Err(KnowledgeError::InvalidReviewState(value)) if value == "mystery")
    );
}

#[test]
fn rejects_claim_without_a_managed_region() {
    let note = "---\npebble_schema: 1\npebble_id: n\ntitle: T\npebble_claims:\n  orphan:\n    status: current\n    review: agent_generated\n---\nbody\n";
    assert!(matches!(
        Note::parse(note),
        Err(KnowledgeError::MissingManagedRegion(claim)) if claim == "orphan"
    ));
}

#[test]
fn rejects_managed_region_for_unknown_claim() {
    let note = "---\npebble_schema: 1\npebble_id: n\ntitle: T\npebble_claims: {}\n---\n<!-- pebble:managed claim=ghost -->\nx\n<!-- /pebble:managed -->\n";
    assert!(matches!(
        Note::parse(note),
        Err(KnowledgeError::UnknownClaim(claim)) if claim == "ghost"
    ));
}

fn anchor_key(anchor: &Anchor) -> AnchorKey {
    AnchorKey::from_anchor(anchor)
}

fn explicit(repo: &str, path: &str, symbol: &str) -> Anchor {
    Anchor::Explicit {
        repo: repo.to_owned(),
        path: path.to_owned(),
        symbol: symbol.to_owned(),
    }
}

fn inferred(repo: &str, path: &str, symbol: &str, confidence: f32) -> Anchor {
    Anchor::Inferred {
        repo: repo.to_owned(),
        path: path.to_owned(),
        symbol: symbol.to_owned(),
        confidence,
        provenance: "test".to_owned(),
    }
}

#[test]
fn direct_explicit_anchor_becomes_pending_update() {
    let anchor = explicit("pebble", "src/lib.rs", "run");
    let mut affected = HashMap::new();
    affected.insert(anchor_key(&anchor), Resolution::DirectlyAffected);
    let input = ImpactInput {
        current_status: Some(ClaimStatus::Current),
        anchors: vec![anchor],
        affected,
    };
    assert_eq!(transition(&input), ClaimStatus::PendingUpdate);
}

#[test]
fn direct_inferred_anchor_thresholds_match_the_documented_table() {
    let cases = [
        (0.85_f32, ClaimStatus::PendingUpdate),
        (0.84, ClaimStatus::Stale),
        (0.60, ClaimStatus::Stale),
        (0.59, ClaimStatus::Current),
    ];
    for (confidence, expected) in cases {
        let anchor = inferred("pebble", "src/lib.rs", "run", confidence);
        let mut affected = HashMap::new();
        affected.insert(anchor_key(&anchor), Resolution::DirectlyAffected);
        let input = ImpactInput {
            current_status: Some(ClaimStatus::Current),
            anchors: vec![anchor],
            affected,
        };
        assert_eq!(transition(&input), expected, "confidence {confidence}");
    }
}

#[test]
fn transitive_impact_becomes_stale_for_explicit_and_high_confidence_inferred() {
    for anchor in [
        explicit("pebble", "src/lib.rs", "run"),
        inferred("pebble", "src/lib.rs", "run", 0.70),
    ] {
        let mut affected = HashMap::new();
        affected.insert(anchor_key(&anchor), Resolution::TransitivelyImpacted);
        let input = ImpactInput {
            current_status: Some(ClaimStatus::Current),
            anchors: vec![anchor],
            affected,
        };
        assert_eq!(transition(&input), ClaimStatus::Stale);
    }
}

#[test]
fn low_confidence_inferred_anchor_is_never_a_dependency() {
    for resolution in [
        Resolution::DirectlyAffected,
        Resolution::TransitivelyImpacted,
    ] {
        let anchor = inferred("pebble", "src/lib.rs", "run", 0.10);
        let mut affected = HashMap::new();
        affected.insert(anchor_key(&anchor), resolution);
        let input = ImpactInput {
            current_status: Some(ClaimStatus::Stale),
            anchors: vec![anchor],
            affected,
        };
        assert_eq!(
            transition(&input),
            ClaimStatus::Stale,
            "retains prior state as a hint only"
        );
    }
}

#[test]
fn unresolvable_explicit_anchor_becomes_broken() {
    let anchor = explicit("pebble", "src/lib.rs", "run");
    let mut affected = HashMap::new();
    affected.insert(anchor_key(&anchor), Resolution::Unresolvable);
    let input = ImpactInput {
        current_status: Some(ClaimStatus::Current),
        anchors: vec![anchor],
        affected,
    };
    assert_eq!(transition(&input), ClaimStatus::Broken);
}

#[test]
fn explicit_outranks_inferred_within_the_same_claim() {
    let broken_anchor = explicit("pebble", "src/lib.rs", "run");
    let hint_anchor = inferred("pebble", "src/other.rs", "helper", 0.10);
    let pending_anchor = explicit("pebble", "src/lib.rs", "run2");
    let low_inferred = inferred("pebble", "src/lib.rs", "run2", 0.30);

    let mut affected = HashMap::new();
    affected.insert(anchor_key(&broken_anchor), Resolution::Unresolvable);
    affected.insert(anchor_key(&hint_anchor), Resolution::DirectlyAffected);
    let input = ImpactInput {
        current_status: Some(ClaimStatus::Current),
        anchors: vec![broken_anchor, hint_anchor],
        affected,
    };
    assert_eq!(transition(&input), ClaimStatus::Broken);

    let mut affected = HashMap::new();
    affected.insert(anchor_key(&pending_anchor), Resolution::DirectlyAffected);
    affected.insert(anchor_key(&low_inferred), Resolution::DirectlyAffected);
    let input = ImpactInput {
        current_status: Some(ClaimStatus::Current),
        anchors: vec![pending_anchor, low_inferred],
        affected,
    };
    assert_eq!(transition(&input), ClaimStatus::PendingUpdate);
}

#[test]
fn unaffected_claims_retain_their_prior_state() {
    let anchor = explicit("pebble", "src/lib.rs", "run");
    let input = ImpactInput {
        current_status: Some(ClaimStatus::Stale),
        anchors: vec![anchor],
        affected: HashMap::new(),
    };
    assert_eq!(transition(&input), ClaimStatus::Stale);
}
