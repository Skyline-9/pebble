//! Queued living-note update packets backed by a dedicated `SQLite` file.
//!
//! Per the source-of-truth model, `updates.db` is separate from the main
//! evidence graph and stores queued corrections until a coding-agent session
//! proposes and validates replacement prose for one managed region.

mod citations;
mod rows;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use super::note::{ClaimStatus, Note, ReviewState};
use crate::domain::{GenerationId, WorktreeRevision};
use crate::knowledge::KnowledgeError;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS update_queue (
    note_path TEXT NOT NULL,
    claim_id TEXT NOT NULL,
    old_claim_snapshot TEXT NOT NULL,
    changed_evidence_ids TEXT NOT NULL,
    broken_citations TEXT NOT NULL,
    allowed_edit_start INTEGER NOT NULL,
    allowed_edit_end INTEGER NOT NULL,
    generation_id TEXT NOT NULL,
    worktree_base_oid TEXT NOT NULL,
    worktree_dirty_digest TEXT,
    queued_at INTEGER NOT NULL,
    PRIMARY KEY (note_path, claim_id)
);
";

const COLUMNS: &str = "note_path, claim_id, old_claim_snapshot, changed_evidence_ids,
     broken_citations, allowed_edit_start, allowed_edit_end, generation_id,
     worktree_base_oid, worktree_dirty_digest, queued_at";

/// One queued correction awaiting agent-generated replacement prose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedUpdate {
    /// Claim requiring replacement prose.
    pub claim_id: String,
    /// Filesystem path of the note containing the claim.
    pub note_path: PathBuf,
    /// Snapshot of the claim's status, review, and body before this change.
    pub old_claim_snapshot: String,
    /// Stable IDs of the evidence that changed and triggered this packet.
    pub changed_evidence_ids: Vec<String>,
    /// Citations from the claim's prior body that no longer resolve.
    pub broken_citations: Vec<String>,
    /// Start byte of the managed region a proposed patch may rewrite.
    pub allowed_edit_start: usize,
    /// End byte of the managed region a proposed patch may rewrite.
    pub allowed_edit_end: usize,
    /// Index generation this packet was computed against.
    pub generation_id: GenerationId,
    /// Worktree revision this packet was computed against.
    pub worktree_revision: WorktreeRevision,
    /// Unix-epoch seconds when this packet was queued.
    pub queued_at: u64,
}

/// One entity/path/symbol citation a proposed patch is allowed to reference.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AllowedCitation {
    /// Canonical repository ID.
    pub repo: String,
    /// Repository-relative cited path.
    pub path: String,
    /// Cited symbol name.
    pub symbol: String,
}

/// Caller-supplied context required to validate and apply a queued update.
pub struct ApplyContext<'a> {
    /// The caller's current index generation.
    pub generation_id: &'a GenerationId,
    /// The caller's current worktree revision.
    pub worktree_revision: &'a WorktreeRevision,
    /// Evidence a proposed patch's citations are allowed to reference.
    pub allowed_evidence: &'a HashSet<AllowedCitation>,
    /// Status to record for the claim once the patch is applied.
    pub new_status: ClaimStatus,
    /// Review state to record for the claim, when the caller changes it.
    pub new_review: Option<ReviewState>,
}

/// Result of successfully applying one queued update to a note on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedPatch {
    /// Claim the patch was applied to.
    pub claim_id: String,
    /// Filesystem path of the rewritten note.
    pub note_path: PathBuf,
    /// The note's full text after the patch was applied.
    pub note_text: String,
}

/// A `SQLite`-backed queue of pending living-note update packets.
pub struct UpdateQueue {
    connection: Connection,
}

impl UpdateQueue {
    /// Open or create the update queue at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error when the parent directory cannot be created or the
    /// database cannot be opened or migrated.
    pub fn open(path: &Path) -> Result<Self, KnowledgeError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self { connection })
    }

    /// Queue a correction, replacing any existing packet for the same claim.
    ///
    /// # Errors
    ///
    /// Returns an error when the packet cannot be serialized or written.
    pub fn enqueue(&self, update: &QueuedUpdate) -> Result<(), KnowledgeError> {
        let changed = serde_json::to_string(&update.changed_evidence_ids)?;
        let broken = serde_json::to_string(&update.broken_citations)?;
        let sql = format!(
            "INSERT INTO update_queue ({COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(note_path, claim_id) DO UPDATE SET
                old_claim_snapshot = excluded.old_claim_snapshot,
                changed_evidence_ids = excluded.changed_evidence_ids,
                broken_citations = excluded.broken_citations,
                allowed_edit_start = excluded.allowed_edit_start,
                allowed_edit_end = excluded.allowed_edit_end,
                generation_id = excluded.generation_id,
                worktree_base_oid = excluded.worktree_base_oid,
                worktree_dirty_digest = excluded.worktree_dirty_digest,
                queued_at = excluded.queued_at"
        );
        self.connection.execute(
            &sql,
            params![
                rows::path_str(&update.note_path)?,
                update.claim_id,
                update.old_claim_snapshot,
                changed,
                broken,
                rows::to_i64(update.allowed_edit_start)?,
                rows::to_i64(update.allowed_edit_end)?,
                update.generation_id.as_str(),
                update.worktree_revision.base_oid(),
                update.worktree_revision.dirty_digest(),
                i64::try_from(update.queued_at).map_err(|_| KnowledgeError::CorruptQueueRow(
                    "queued_at overflow".to_owned()
                ))?,
            ],
        )?;
        Ok(())
    }

    /// Return every queued update, ordered by queue time.
    ///
    /// # Errors
    ///
    /// Returns an error when the queue cannot be read or a row is corrupt.
    pub fn list(&self) -> Result<Vec<QueuedUpdate>, KnowledgeError> {
        let sql =
            format!("SELECT {COLUMNS} FROM update_queue ORDER BY queued_at, note_path, claim_id");
        let mut statement = self.connection.prepare(&sql)?;
        let raw_rows = statement.query_map([], rows::row_to_raw)?;
        raw_rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(rows::finish)
            .collect()
    }

    fn find(
        &self,
        note_path: &Path,
        claim_id: &str,
    ) -> Result<Option<QueuedUpdate>, KnowledgeError> {
        let sql =
            format!("SELECT {COLUMNS} FROM update_queue WHERE note_path = ?1 AND claim_id = ?2");
        let raw = self
            .connection
            .query_row(
                &sql,
                params![rows::path_str(note_path)?, claim_id],
                rows::row_to_raw,
            )
            .optional()?;
        raw.map(rows::finish).transpose()
    }

    fn remove(&self, note_path: &Path, claim_id: &str) -> Result<(), KnowledgeError> {
        self.connection.execute(
            "DELETE FROM update_queue WHERE note_path = ?1 AND claim_id = ?2",
            params![rows::path_str(note_path)?, claim_id],
        )?;
        Ok(())
    }

    /// Validate and apply a proposed patch to the queued claim's managed region.
    ///
    /// Rejects the patch and leaves the queue untouched when: no packet is
    /// queued for the claim, the packet's generation or worktree revision no
    /// longer matches `context`, the note's managed region has moved since
    /// queueing, or the patch cites evidence outside `context.allowed_evidence`.
    /// On success, rewrites the note on disk and removes the queued packet.
    ///
    /// # Errors
    ///
    /// Returns an error for any of the rejection conditions above, or when the
    /// note cannot be read, parsed, or written.
    pub fn apply(
        &self,
        note_path: &Path,
        claim_id: &str,
        proposed_patch: &str,
        context: &ApplyContext<'_>,
    ) -> Result<AppliedPatch, KnowledgeError> {
        let queued = self
            .find(note_path, claim_id)?
            .ok_or_else(|| KnowledgeError::QueueEmpty(claim_id.to_owned()))?;
        if &queued.generation_id != context.generation_id
            || &queued.worktree_revision != context.worktree_revision
        {
            return Err(KnowledgeError::StaleGeneration);
        }

        let raw = fs::read_to_string(note_path)?;
        let note = Note::parse(&raw)?;
        let region = note
            .managed_region(claim_id)
            .ok_or_else(|| KnowledgeError::ClaimNotFound(claim_id.to_owned()))?;
        let current = region.content_range();
        if current.start != queued.allowed_edit_start || current.end != queued.allowed_edit_end {
            return Err(KnowledgeError::RegionMismatch);
        }

        citations::validate_citations(proposed_patch, context.allowed_evidence)?;
        let note_text = note.apply_claim_patch(
            claim_id,
            proposed_patch,
            context.new_status,
            context.new_review,
        )?;
        fs::write(note_path, &note_text)?;
        self.remove(note_path, claim_id)?;

        Ok(AppliedPatch {
            claim_id: claim_id.to_owned(),
            note_path: note_path.to_owned(),
            note_text,
        })
    }
}
