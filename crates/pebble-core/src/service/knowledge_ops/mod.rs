//! Living-knowledge note listing, reading, and queued-update application.

mod citations;
mod errors;

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::domain::RepositoryId;
use crate::index::GenerationReader;
use crate::knowledge::{ApplyContext, ClaimStatus, Note, QueuedUpdate, UpdateQueue};
use crate::repository::SystemGit;

use super::health::{map_index_unavailable, map_repository};
use super::{PebbleService, ServiceError};
use citations::collect_allowed_citations;
use errors::map_knowledge;

/// Largest number of note files scanned per repository.
const MAX_NOTE_FILES: usize = 10_000;
/// Largest accepted note file size.
const MAX_NOTE_BYTES: u64 = 4 * 1024 * 1024;

/// One managed claim summarized for listing.
#[derive(Clone, Debug, Serialize)]
pub struct NoteClaimSummary {
    /// Filesystem path of the note containing the claim.
    pub note_path: PathBuf,
    /// Claim identifier.
    pub claim_id: String,
    /// Claim's current status token.
    pub status: String,
    /// Claim's current review-state token.
    pub review: String,
}

/// One managed claim's full detail, including its current managed body text.
#[derive(Clone, Debug, Serialize)]
pub struct NoteClaimDetail {
    /// Filesystem path of the note containing the claim.
    pub note_path: PathBuf,
    /// Claim identifier.
    pub claim_id: String,
    /// Claim's current status token.
    pub status: String,
    /// Claim's current review-state token.
    pub review: String,
    /// Claim's current managed-region body text.
    pub body: String,
}

/// One queued update packet summarized for listing.
#[derive(Clone, Debug, Serialize)]
pub struct QueuedUpdateSummary {
    /// Claim requiring replacement prose.
    pub claim_id: String,
    /// Filesystem path of the note containing the claim.
    pub note_path: PathBuf,
    /// Stable IDs of the evidence that changed and triggered this packet.
    pub changed_evidence_ids: Vec<String>,
    /// Citations from the claim's prior body that no longer resolve.
    pub broken_citations: Vec<String>,
    /// Index generation this packet was computed against.
    pub generation_id: String,
    /// Worktree revision this packet was computed against.
    pub worktree_revision: String,
    /// Unix-epoch seconds when this packet was queued.
    pub queued_at: u64,
}

impl From<&QueuedUpdate> for QueuedUpdateSummary {
    fn from(update: &QueuedUpdate) -> Self {
        Self {
            claim_id: update.claim_id.clone(),
            note_path: update.note_path.clone(),
            changed_evidence_ids: update.changed_evidence_ids.clone(),
            broken_citations: update.broken_citations.clone(),
            generation_id: update.generation_id.as_str().to_owned(),
            worktree_revision: update.worktree_revision.to_string(),
            queued_at: update.queued_at,
        }
    }
}

/// Result of applying a queued patch to one claim's managed region.
#[derive(Clone, Debug, Serialize)]
pub struct AppliedPatchResult {
    /// Claim the patch was applied to.
    pub claim_id: String,
    /// Filesystem path of the rewritten note.
    pub note_path: PathBuf,
}

impl PebbleService {
    /// List every managed claim across a registered checkout's living
    /// knowledge notes, optionally filtered by status.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error for an unrecognized `status` token,
    /// an unavailable-evidence error when the repository is not registered,
    /// or an operational error when a note cannot be read or parsed.
    pub fn note_list(
        &self,
        repository: &RepositoryId,
        status: Option<&str>,
    ) -> Result<Vec<NoteClaimSummary>, ServiceError> {
        let status_filter = status
            .map(ClaimStatus::try_from)
            .transpose()
            .map_err(ServiceError::usage)?;
        let mut summaries = Vec::new();
        for (path, note) in self.load_notes(repository)? {
            for (claim_id, claim) in note.claims() {
                if status_filter.is_some_and(|filter| filter != claim.status()) {
                    continue;
                }
                summaries.push(NoteClaimSummary {
                    note_path: path.clone(),
                    claim_id: claim_id.clone(),
                    status: claim.status().as_str().to_owned(),
                    review: claim.review().as_str().to_owned(),
                });
            }
        }
        Ok(summaries)
    }

    /// Read one claim's full detail from a registered checkout's living
    /// knowledge notes.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error when the claim does not exist, or an
    /// operational error when a note cannot be read or parsed.
    pub fn note_read(
        &self,
        repository: &RepositoryId,
        claim_id: &str,
    ) -> Result<NoteClaimDetail, ServiceError> {
        for (path, note) in self.load_notes(repository)? {
            if let Some(claim) = note.claim(claim_id) {
                let body = note.managed_body(claim_id).unwrap_or_default().to_owned();
                return Ok(NoteClaimDetail {
                    note_path: path,
                    claim_id: claim_id.to_owned(),
                    status: claim.status().as_str().to_owned(),
                    review: claim.review().as_str().to_owned(),
                    body,
                });
            }
        }
        Err(ServiceError::usage(format!(
            "claim {claim_id} was not found"
        )))
    }

    /// List every queued living-note update packet for a registered checkout.
    ///
    /// # Errors
    ///
    /// Returns an unavailable-evidence error when the repository is not
    /// registered, or an operational error when the queue cannot be read.
    pub fn update_list(
        &self,
        repository: &RepositoryId,
    ) -> Result<Vec<QueuedUpdateSummary>, ServiceError> {
        let queue = self.update_queue(repository)?;
        let updates = queue.list().map_err(map_knowledge)?;
        Ok(updates.iter().map(QueuedUpdateSummary::from).collect())
    }

    /// Validate and apply one queued patch to a claim's managed region.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error when no packet is queued for the
    /// claim or the patch cites evidence outside the current generation's
    /// allow-list, a stale-evidence error when the queued packet or managed
    /// region no longer matches the current generation, or an operational
    /// error when the note cannot be read or written.
    pub fn update_apply(
        &self,
        repository: &RepositoryId,
        claim_id: &str,
        patch: &str,
    ) -> Result<AppliedPatchResult, ServiceError> {
        let registration = self.registration(repository)?;
        let queue = self.update_queue(repository)?;
        let queued = queue
            .list()
            .map_err(map_knowledge)?
            .into_iter()
            .find(|update| update.claim_id == claim_id)
            .ok_or_else(|| {
                ServiceError::usage(format!("no queued update exists for claim {claim_id}"))
            })?;
        let reader = GenerationReader::open_current(&self.layout.generations(repository))
            .map_err(map_index_unavailable)?;
        let git = SystemGit::discover().map_err(map_repository)?;
        let worktree_revision = git
            .revision(registration.checkout())
            .map_err(map_repository)?;
        let allowed_evidence = collect_allowed_citations(&reader)?;
        let context = ApplyContext {
            generation_id: reader.id(),
            worktree_revision: &worktree_revision,
            allowed_evidence: &allowed_evidence,
            new_status: ClaimStatus::Current,
            new_review: None,
        };
        let applied = queue
            .apply(&queued.note_path, claim_id, patch, &context)
            .map_err(map_knowledge)?;
        Ok(AppliedPatchResult {
            claim_id: applied.claim_id,
            note_path: applied.note_path,
        })
    }

    fn load_notes(&self, repository: &RepositoryId) -> Result<Vec<(PathBuf, Note)>, ServiceError> {
        let registration = self.registration(repository)?;
        let directory = registration.checkout().join(".pebble").join("knowledge");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(ServiceError::operational(error)),
        };
        let mut notes = Vec::new();
        for entry in entries {
            if notes.len() >= MAX_NOTE_FILES {
                break;
            }
            let entry = entry.map_err(ServiceError::operational)?;
            let path = entry.path();
            if path.extension().and_then(OsStr::to_str) != Some("md") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(ServiceError::operational)?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() > MAX_NOTE_BYTES
            {
                continue;
            }
            let raw = fs::read_to_string(&path).map_err(ServiceError::operational)?;
            let note = Note::parse(&raw).map_err(map_knowledge)?;
            notes.push((path, note));
        }
        notes.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(notes)
    }

    fn update_queue(&self, repository: &RepositoryId) -> Result<UpdateQueue, ServiceError> {
        self.registration(repository)?;
        let path = self.repository_root(repository).join("updates.db");
        UpdateQueue::open(&path).map_err(map_knowledge)
    }
}
