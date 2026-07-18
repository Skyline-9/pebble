//! Classifying [`KnowledgeError`] into the service's failure taxonomy.

use crate::knowledge::KnowledgeError;

use super::super::ServiceError;

pub(super) fn map_knowledge(error: KnowledgeError) -> ServiceError {
    match error {
        KnowledgeError::QueueEmpty(_)
        | KnowledgeError::CitationNotAllowed(_)
        | KnowledgeError::ClaimNotFound(_)
        | KnowledgeError::InvalidClaimStatus(_)
        | KnowledgeError::InvalidReviewState(_) => ServiceError::usage(error),
        KnowledgeError::StaleGeneration | KnowledgeError::RegionMismatch => {
            ServiceError::stale(error)
        }
        KnowledgeError::Io(_)
        | KnowledgeError::Sqlite(_)
        | KnowledgeError::Json(_)
        | KnowledgeError::Domain(_)
        | KnowledgeError::MissingFrontmatter
        | KnowledgeError::UnclosedFrontmatter
        | KnowledgeError::MalformedFrontmatter(_)
        | KnowledgeError::MalformedManagedMarker
        | KnowledgeError::UnclosedManagedRegion(_)
        | KnowledgeError::UnknownClaim(_)
        | KnowledgeError::DuplicateManagedRegion(_)
        | KnowledgeError::MissingManagedRegion(_)
        | KnowledgeError::OverlappingEdit
        | KnowledgeError::CorruptQueueRow(_)
        | KnowledgeError::NonUtf8Path => ServiceError::operational(error),
    }
}
