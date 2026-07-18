//! Worktree revision identity.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::DomainError;

/// A Git base object ID plus an optional digest of dirty worktree contents.
///
/// Invariant-bearing values are exposed only through read-only accessors.
///
/// ```compile_fail
/// use pebble_core::domain::WorktreeRevision;
///
/// let mut revision = WorktreeRevision::clean("0123456789abcdef")?;
/// revision.base_oid = "invalid".to_owned();
/// # Ok::<(), pebble_core::error::DomainError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct WorktreeRevision {
    /// Commit object ID on which the indexed snapshot is based.
    base_oid: String,
    /// Deterministic digest of dirty tracked and untracked contents.
    dirty_digest: Option<String>,
}

#[derive(Deserialize)]
struct RevisionWire {
    base_oid: String,
    dirty_digest: Option<String>,
}

impl<'de> Deserialize<'de> for WorktreeRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RevisionWire::deserialize(deserializer)?;
        match wire.dirty_digest {
            Some(digest) => Self::dirty(wire.base_oid, digest),
            None => Self::clean(wire.base_oid),
        }
        .map_err(serde::de::Error::custom)
    }
}

impl WorktreeRevision {
    /// Construct a clean worktree revision.
    ///
    /// # Errors
    ///
    /// Returns an error when `base_oid` is empty or not lowercase hexadecimal.
    pub fn clean(base_oid: impl Into<String>) -> Result<Self, DomainError> {
        let base_oid = base_oid.into();
        validate_hex(&base_oid, "base OID")?;
        Ok(Self {
            base_oid,
            dirty_digest: None,
        })
    }

    /// Construct a dirty worktree revision.
    ///
    /// # Errors
    ///
    /// Returns an error when either component is empty or not lowercase hexadecimal.
    pub fn dirty(
        base_oid: impl Into<String>,
        dirty_digest: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let base_oid = base_oid.into();
        validate_hex(&base_oid, "base OID")?;
        let dirty_digest = dirty_digest.into();
        validate_hex(&dirty_digest, "dirty digest")?;
        Ok(Self {
            base_oid,
            dirty_digest: Some(dirty_digest),
        })
    }

    /// Return the validated lowercase hexadecimal base object ID.
    #[must_use]
    pub fn base_oid(&self) -> &str {
        &self.base_oid
    }

    /// Return the validated lowercase hexadecimal dirty-worktree digest, when present.
    #[must_use]
    pub fn dirty_digest(&self) -> Option<&str> {
        self.dirty_digest.as_deref()
    }
}

impl fmt::Display for WorktreeRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.base_oid)?;
        if let Some(digest) = &self.dirty_digest {
            write!(formatter, "+dirty.{digest}")?;
        }
        Ok(())
    }
}

fn validate_hex(value: &str, field: &'static str) -> Result<(), DomainError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(DomainError::InvalidRevision { field });
    }
    Ok(())
}
