//! Managed living-knowledge notes, claim state, and queued update packets.
//!
//! Notes are ordinary Markdown files with a YAML frontmatter block and zero or
//! more `<!-- pebble:managed claim=ID -->` regions. Pebble tracks each managed
//! claim's anchors to source code and, on relevant code change, transitions
//! its status and queues an update packet for the next coding-agent session.

mod claims;
mod note;
mod updates;

use thiserror::Error;

use crate::error::DomainError;

pub use claims::{AnchorKey, ImpactInput, Resolution, transition};
pub use note::{Anchor, ClaimStatus, ManagedRegion, Note, NoteClaim, ReviewState};
pub use updates::{AllowedCitation, AppliedPatch, ApplyContext, QueuedUpdate, UpdateQueue};

/// Failure while parsing, mutating, or persisting managed living-knowledge notes.
#[derive(Debug, Error)]
pub enum KnowledgeError {
    /// A local filesystem operation failed.
    #[error("knowledge filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// A `SQLite` operation on the update queue failed.
    #[error("update queue SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Serializing or deserializing queued update metadata failed.
    #[error("update queue JSON payload is invalid: {0}")]
    Json(#[from] serde_json::Error),
    /// A stable domain value failed validation.
    #[error("knowledge domain value is invalid: {0}")]
    Domain(#[from] DomainError),
    /// The note did not start with a `---` frontmatter delimiter.
    #[error("note is missing a YAML frontmatter block")]
    MissingFrontmatter,
    /// The note's frontmatter block was never closed with a second `---` delimiter.
    #[error("note frontmatter block is never closed")]
    UnclosedFrontmatter,
    /// The frontmatter YAML failed to parse or did not match the documented schema.
    #[error("note frontmatter is malformed: {0}")]
    MalformedFrontmatter(String),
    /// A `pebble_claims` status token was not one of the documented values.
    #[error("claim status '{0}' is not a recognized Pebble claim status")]
    InvalidClaimStatus(String),
    /// A `pebble_claims` review token was not one of the documented values.
    #[error("review state '{0}' is not a recognized Pebble review state")]
    InvalidReviewState(String),
    /// A managed-region marker was malformed or improperly nested.
    #[error("managed-region marker is malformed")]
    MalformedManagedMarker,
    /// A managed region opened but was never closed.
    #[error("managed region for claim '{0}' is never closed")]
    UnclosedManagedRegion(String),
    /// A managed region referenced a claim absent from frontmatter.
    #[error("managed region references unknown claim '{0}'")]
    UnknownClaim(String),
    /// The same claim ID had more than one managed region.
    #[error("claim '{0}' has more than one managed region")]
    DuplicateManagedRegion(String),
    /// A frontmatter claim had no corresponding managed region in the body.
    #[error("claim '{0}' has no managed region in the note body")]
    MissingManagedRegion(String),
    /// The requested claim does not exist in this note.
    #[error("claim '{0}' does not exist in this note")]
    ClaimNotFound(String),
    /// Two computed edit ranges overlapped.
    #[error("computed note edits overlap")]
    OverlappingEdit,
    /// No queued update packet exists for the requested claim.
    #[error("no queued update exists for claim '{0}'")]
    QueueEmpty(String),
    /// The queued packet's generation or worktree revision no longer matches the caller's.
    #[error("queued update targets a stale generation or worktree revision; regenerate it")]
    StaleGeneration,
    /// The note's managed region moved since the update was queued.
    #[error("managed region for the queued claim has moved; regenerate the update")]
    RegionMismatch,
    /// A proposed patch cited a path or symbol absent from the allowed evidence set.
    #[error("proposed patch cites evidence outside the allow-list: {0}")]
    CitationNotAllowed(String),
    /// A stored update-queue row contained an out-of-range or corrupt value.
    #[error("update queue row is corrupt: {0}")]
    CorruptQueueRow(String),
    /// A path could not be represented as UTF-8 text.
    #[error("path is not valid UTF-8")]
    NonUtf8Path,
}
