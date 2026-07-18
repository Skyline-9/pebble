//! Versioned local-state path layout.

use std::path::{Path, PathBuf};

use crate::domain::RepositoryId;

/// Paths for Pebble's disposable local repository projections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateLayout {
    root: PathBuf,
}

impl StateLayout {
    /// Build the version-one state layout beneath a user's home directory.
    #[must_use]
    pub fn new(home: &Path) -> Self {
        Self {
            root: home.join(".pebble").join("v1"),
        }
    }

    /// Return the root of the versioned local state tree.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the immutable-generation directory for one repository.
    #[must_use]
    pub fn generations(&self, repository: &RepositoryId) -> PathBuf {
        self.root
            .join("repos")
            .join(repository.as_str())
            .join("generations")
    }
}
