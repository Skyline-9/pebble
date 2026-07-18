//! `SQLite` row (de)serialization for queued update packets.

use std::path::{Path, PathBuf};

use rusqlite::Row;

use super::QueuedUpdate;
use crate::domain::{GenerationId, WorktreeRevision};
use crate::knowledge::KnowledgeError;

pub(super) type RawRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
    Option<String>,
    i64,
);

pub(super) fn row_to_raw(row: &Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}

pub(super) fn finish(raw: RawRow) -> Result<QueuedUpdate, KnowledgeError> {
    let (
        note_path,
        claim_id,
        old_claim_snapshot,
        changed_evidence_ids,
        broken_citations,
        allowed_edit_start,
        allowed_edit_end,
        generation_id,
        worktree_base_oid,
        worktree_dirty_digest,
        queued_at,
    ) = raw;
    let worktree_revision = match worktree_dirty_digest {
        Some(digest) => WorktreeRevision::dirty(worktree_base_oid, digest),
        None => WorktreeRevision::clean(worktree_base_oid),
    }?;
    Ok(QueuedUpdate {
        claim_id,
        note_path: PathBuf::from(note_path),
        old_claim_snapshot,
        changed_evidence_ids: serde_json::from_str(&changed_evidence_ids)?,
        broken_citations: serde_json::from_str(&broken_citations)?,
        allowed_edit_start: from_i64(allowed_edit_start)?,
        allowed_edit_end: from_i64(allowed_edit_end)?,
        generation_id: GenerationId::try_from(generation_id)?,
        worktree_revision,
        queued_at: u64::try_from(queued_at)
            .map_err(|_| KnowledgeError::CorruptQueueRow("queued_at is negative".to_owned()))?,
    })
}

pub(super) fn to_i64(value: usize) -> Result<i64, KnowledgeError> {
    i64::try_from(value)
        .map_err(|_| KnowledgeError::CorruptQueueRow("edit offset overflow".to_owned()))
}

fn from_i64(value: i64) -> Result<usize, KnowledgeError> {
    usize::try_from(value)
        .map_err(|_| KnowledgeError::CorruptQueueRow("edit offset is negative".to_owned()))
}

pub(super) fn path_str(path: &Path) -> Result<&str, KnowledgeError> {
    path.to_str().ok_or(KnowledgeError::NonUtf8Path)
}
