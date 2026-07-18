#![forbid(unsafe_code)]

//! End-to-end living-note mutation scenarios (design doc section 11.3).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::domain::{GenerationId, WorktreeRevision};
use pebble_core::knowledge::{
    AllowedCitation, ApplyContext, ClaimStatus, KnowledgeError, Note, QueuedUpdate, ReviewState,
    UpdateQueue,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-living-notes-{label}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_note() -> String {
    r"---
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
"
    .to_owned()
}

fn write_note(dir: &TempDir) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = dir.path().join("authentication.md");
    fs::write(&path, fixture_note())?;
    Ok(path)
}

fn queue_session_validation(
    note_path: &Path,
    generation: &GenerationId,
    revision: &WorktreeRevision,
) -> Result<QueuedUpdate, Box<dyn std::error::Error>> {
    let raw = fs::read_to_string(note_path)?;
    let note = Note::parse(&raw)?;
    let region = note
        .managed_region("session-validation")
        .ok_or("missing managed region")?;
    let range = region.content_range();
    Ok(QueuedUpdate {
        claim_id: "session-validation".to_owned(),
        note_path: note_path.to_owned(),
        old_claim_snapshot: note.claim_snapshot("session-validation")?,
        changed_evidence_ids: vec!["evidence-session-1".to_owned()],
        broken_citations: Vec::new(),
        allowed_edit_start: range.start,
        allowed_edit_end: range.end,
        generation_id: generation.clone(),
        worktree_revision: revision.clone(),
        queued_at: 1,
    })
}

fn allow_list() -> HashSet<AllowedCitation> {
    HashSet::from([AllowedCitation {
        repo: "pebble".to_owned(),
        path: "src/auth/session.rs".to_owned(),
        symbol: "validate_session".to_owned(),
    }])
}

#[test]
fn correct_claim_enters_the_queue_while_unrelated_claim_stays_untouched()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("enters-queue")?;
    let note_path = write_note(&dir)?;
    let generation = GenerationId::try_from("gen-1".to_owned())?;
    let revision = WorktreeRevision::clean("2f0c38a0000000000000000000000000000000")?;
    let queue = UpdateQueue::open(&dir.path().join("updates.db"))?;

    queue.enqueue(&queue_session_validation(
        &note_path,
        &generation,
        &revision,
    )?)?;

    let queued = queue.list()?;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].claim_id, "session-validation");

    let raw = fs::read_to_string(&note_path)?;
    let note = Note::parse(&raw)?;
    assert_eq!(
        note.claim("token-refresh").ok_or("missing claim")?.status(),
        ClaimStatus::Current
    );
    Ok(())
}

#[test]
fn apply_rewrites_only_the_managed_region_and_claim_status()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("apply-success")?;
    let note_path = write_note(&dir)?;
    let generation = GenerationId::try_from("gen-1".to_owned())?;
    let revision = WorktreeRevision::clean("2f0c38a0000000000000000000000000000000")?;
    let queue = UpdateQueue::open(&dir.path().join("updates.db"))?;
    queue.enqueue(&queue_session_validation(
        &note_path,
        &generation,
        &revision,
    )?)?;

    let allowed = allow_list();
    let context = ApplyContext {
        generation_id: &generation,
        worktree_revision: &revision,
        allowed_evidence: &allowed,
        new_status: ClaimStatus::Current,
        new_review: None,
    };
    let new_body = "## Session validation\n\n`validate_session` now also checks MFA state.\n\nSources:\n- `pebble@2f0c38a:src/auth/session.rs#validate_session:L42-L90`\n";
    let applied = queue.apply(&note_path, "session-validation", new_body, &context)?;

    assert_eq!(applied.note_text, fs::read_to_string(&note_path)?);
    assert!(applied.note_text.contains("custom_field: keep-me"));
    assert!(
        applied
            .note_text
            .contains("This paragraph is owned by the author.")
    );
    assert!(
        applied
            .note_text
            .contains("## More human prose here, untouched.")
    );
    assert!(
        applied
            .note_text
            .contains("`refresh_token` rotates the session token.")
    );

    let note = Note::parse(&applied.note_text)?;
    let session = note.claim("session-validation").ok_or("missing claim")?;
    assert_eq!(session.status(), ClaimStatus::Current);
    assert_eq!(session.review(), ReviewState::AgentGenerated);
    assert_eq!(note.managed_body("session-validation"), Some(new_body));

    let token = note.claim("token-refresh").ok_or("missing claim")?;
    assert_eq!(token.status(), ClaimStatus::Current);

    assert!(queue.list()?.is_empty());
    Ok(())
}

#[test]
fn apply_rejects_citations_outside_the_allow_list() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("apply-citation-reject")?;
    let note_path = write_note(&dir)?;
    let generation = GenerationId::try_from("gen-1".to_owned())?;
    let revision = WorktreeRevision::clean("2f0c38a0000000000000000000000000000000")?;
    let queue = UpdateQueue::open(&dir.path().join("updates.db"))?;
    queue.enqueue(&queue_session_validation(
        &note_path,
        &generation,
        &revision,
    )?)?;

    let allowed = allow_list();
    let context = ApplyContext {
        generation_id: &generation,
        worktree_revision: &revision,
        allowed_evidence: &allowed,
        new_status: ClaimStatus::Current,
        new_review: None,
    };
    let smuggled_body =
        "## Session validation\n\nSee `pebble@2f0c38a:src/auth/other.rs#other_symbol:L1-L2`.\n";
    let original = fs::read_to_string(&note_path)?;

    let result = queue.apply(&note_path, "session-validation", smuggled_body, &context);
    assert!(matches!(result, Err(KnowledgeError::CitationNotAllowed(_))));
    assert_eq!(
        fs::read_to_string(&note_path)?,
        original,
        "rejected patch must not touch the note"
    );
    assert_eq!(
        queue.list()?.len(),
        1,
        "rejected patch must not drain the queue"
    );
    Ok(())
}

#[test]
fn apply_rejects_a_stale_generation_or_worktree_and_requires_regeneration()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("apply-stale")?;
    let note_path = write_note(&dir)?;
    let queued_generation = GenerationId::try_from("gen-1".to_owned())?;
    let queued_revision = WorktreeRevision::clean("2f0c38a0000000000000000000000000000000")?;
    let queue = UpdateQueue::open(&dir.path().join("updates.db"))?;
    queue.enqueue(&queue_session_validation(
        &note_path,
        &queued_generation,
        &queued_revision,
    )?)?;

    let stale_generation = GenerationId::try_from("gen-2".to_owned())?;
    let allowed = allow_list();
    let context = ApplyContext {
        generation_id: &stale_generation,
        worktree_revision: &queued_revision,
        allowed_evidence: &allowed,
        new_status: ClaimStatus::Current,
        new_review: None,
    };
    let original = fs::read_to_string(&note_path)?;

    let result = queue.apply(
        &note_path,
        "session-validation",
        "## Session validation\n",
        &context,
    );
    assert!(matches!(result, Err(KnowledgeError::StaleGeneration)));
    assert_eq!(
        fs::read_to_string(&note_path)?,
        original,
        "a stale packet must not touch the note"
    );

    let queued = queue.list()?;
    assert_eq!(
        queued.len(),
        1,
        "the queue must retain the packet for regeneration"
    );
    assert_eq!(queued[0].generation_id, queued_generation);
    Ok(())
}
