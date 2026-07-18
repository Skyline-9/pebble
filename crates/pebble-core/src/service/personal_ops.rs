//! Personal knowledge notes and promotion into a registered repository.

use std::path::PathBuf;

use serde::Serialize;

use crate::domain::RepositoryId;
use crate::personal::{PersonalError, PersonalNote, PersonalNoteStore, promote, promote_confirmed};

use super::{PebbleService, ServiceError};

/// One personal knowledge note summarized for creation or listing.
#[derive(Clone, Debug, Serialize)]
pub struct PersonalNoteSummary {
    /// Stable note identifier.
    pub id: String,
    /// Note title.
    pub title: String,
    /// Local filesystem path the note is stored at.
    pub path: PathBuf,
}

impl From<&PersonalNote> for PersonalNoteSummary {
    fn from(note: &PersonalNote) -> Self {
        Self {
            id: note.id().to_owned(),
            title: note.title().to_owned(),
            path: note.path().to_owned(),
        }
    }
}

/// Outcome of one promotion request: either a written note, or an unwritten
/// preview to review before confirming.
#[derive(Clone, Debug, Serialize)]
pub struct PersonalPromotionOutcome {
    /// Path the note was, or would be, written to.
    pub destination_path: PathBuf,
    /// Whether the note was actually written to `destination_path`.
    pub applied: bool,
    /// Unified diff against any existing destination content, present only
    /// when nothing was written yet.
    pub diff: Option<String>,
    /// Whether writing would replace different existing content, present
    /// only when nothing was written yet.
    pub would_overwrite: Option<bool>,
}

impl PebbleService {
    /// Create a new personal knowledge note.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error for an empty or invalid title, or an
    /// operational error when the note cannot be written.
    pub fn personal_note_create(&self, title: &str) -> Result<PersonalNoteSummary, ServiceError> {
        let note = PersonalNoteStore::create(&self.personal_root(), title).map_err(map_personal)?;
        Ok(PersonalNoteSummary::from(&note))
    }

    /// List every personal knowledge note.
    ///
    /// # Errors
    ///
    /// Returns an operational error when personal storage cannot be read.
    pub fn personal_note_list(&self) -> Result<Vec<PersonalNoteSummary>, ServiceError> {
        let notes = PersonalNoteStore::list(&self.personal_root()).map_err(map_personal)?;
        Ok(notes.iter().map(PersonalNoteSummary::from).collect())
    }

    /// Preview or apply promoting one personal note into `repository`'s
    /// shared knowledge directory.
    ///
    /// Without `confirm`, nothing is written; the returned outcome carries a
    /// unified diff against any existing destination content for review.
    /// With `confirm`, the note is written, unless the destination already
    /// exists with different content and `acknowledge_overwrite` is not
    /// also `true`.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error when overwrite acknowledgement is
    /// required and absent, or an operational error when the promotion
    /// cannot be written.
    pub fn personal_note_promote(
        &self,
        note_id: &str,
        repository: &RepositoryId,
        confirm: bool,
        acknowledge_overwrite: bool,
    ) -> Result<PersonalPromotionOutcome, ServiceError> {
        let note = PersonalNoteStore::read(&self.personal_root(), note_id).map_err(map_personal)?;
        let registration = self.registration(repository)?;
        if !confirm {
            let diff = promote(&note, registration.checkout()).map_err(map_personal)?;
            return Ok(PersonalPromotionOutcome {
                destination_path: diff.destination_path().to_owned(),
                applied: false,
                diff: Some(diff.unified_diff().to_owned()),
                would_overwrite: Some(diff.would_overwrite()),
            });
        }
        let destination_path = promote_confirmed(
            &note,
            registration.checkout(),
            confirm,
            acknowledge_overwrite,
        )
        .map_err(map_personal)?;
        Ok(PersonalPromotionOutcome {
            destination_path,
            applied: true,
            diff: None,
            would_overwrite: None,
        })
    }

    fn personal_root(&self) -> PathBuf {
        self.state_root().join("personal")
    }
}

fn map_personal(error: PersonalError) -> ServiceError {
    match error {
        PersonalError::EmptyTitle
        | PersonalError::InvalidTitle(_)
        | PersonalError::NotFound(_)
        | PersonalError::InvalidNoteId(_)
        | PersonalError::InvalidSlug(_)
        | PersonalError::NotConfirmed
        | PersonalError::WouldOverwrite(_) => ServiceError::usage(error),
        PersonalError::Io(_)
        | PersonalError::MalformedFrontmatter(_)
        | PersonalError::PathTraversal => ServiceError::operational(error),
    }
}
