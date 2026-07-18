//! Errors returned by Pebble domain contracts.

use thiserror::Error;

/// A domain value failed validation.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DomainError {
    /// An identifier was empty or contained a non-portable character.
    #[error("{kind} must contain only ASCII letters, digits, '.', '_', or '-'")]
    InvalidIdentifier {
        /// Name of the rejected identifier kind.
        kind: &'static str,
    },
    /// A revision component was empty or was not lowercase hexadecimal.
    #[error("{field} must be nonempty lowercase hexadecimal")]
    InvalidRevision {
        /// Name of the rejected revision field.
        field: &'static str,
    },
    /// A citation path was not a normalized repository-relative slash path.
    #[error("citation path must be a normalized repository-relative slash path")]
    InvalidCitationPath,
    /// A citation line range was not one-based and inclusive.
    #[error("citation line range must be one-based and inclusive")]
    InvalidLineRange,
    /// An evidence budget was outside the supported inclusive range.
    #[error("evidence budget must be between {minimum} and {maximum} tokens")]
    InvalidEvidenceBudget {
        /// Smallest supported budget.
        minimum: u32,
        /// Largest supported budget.
        maximum: u32,
    },
}
