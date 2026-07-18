//! Model-free application operations over secure local Pebble state.

mod citation_race;
mod embeddings_ops;
mod health;
mod knowledge_ops;
mod operations;
mod personal_ops;
mod trace_race;
mod workspace_ops;

#[cfg(test)]
#[path = "operations/tests.rs"]
mod operations_tests;

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::domain::{Citation, RepositoryId};
use crate::repository::StateLayout;

pub use embeddings_ops::{ModelInstallResult, ModelRemoval};
pub use health::{HealthCounts, HealthStatus};
pub use knowledge_ops::{
    AppliedPatchResult, NoteClaimDetail, NoteClaimSummary, QueuedUpdateSummary,
};
pub use personal_ops::{PersonalNoteSummary, PersonalPromotionOutcome};
pub use workspace_ops::{WorkspaceSearchHit, WorkspaceSearchOutcome, WorkspaceSummary};

/// Result of initializing one portable repository configuration.
#[derive(Clone, Debug, Serialize)]
pub struct InitResult {
    /// Canonical repository identity persisted in portable configuration.
    pub repository_id: RepositoryId,
    /// Canonical local checkout path.
    pub repository: PathBuf,
    /// Versioned machine-local state root.
    pub state_root: PathBuf,
}

/// Result of registering one local checkout.
#[derive(Clone, Debug, Serialize)]
pub struct RegisterResult {
    /// Canonical repository identity.
    pub repository_id: RepositoryId,
    /// Canonical registered checkout path.
    pub repository: PathBuf,
    /// Whether duplicate identity use was explicitly allowed.
    pub alternate_worktree: bool,
}

/// Result of compiling and atomically activating one immutable generation.
#[derive(Clone, Debug, Serialize)]
pub struct IndexResult {
    /// Canonical repository identity.
    pub repository_id: RepositoryId,
    /// Newly active generation.
    pub generation: String,
    /// Exact indexed worktree revision.
    pub revision: String,
    /// Validated graph row counts.
    pub counts: HealthCounts,
}

/// Exact citation content read from the still-matching worktree revision.
#[derive(Clone, Debug, Serialize)]
pub struct ReadResult {
    /// Citation that was resolved without widening its range.
    pub citation: Citation,
    /// Exact one-based inclusive source lines.
    pub content: String,
}

/// Stable application service bound to one user's versioned local state.
#[derive(Clone, Debug)]
pub struct PebbleService {
    layout: StateLayout,
}

impl PebbleService {
    /// Open the model-free service beneath `home`.
    ///
    /// # Errors
    ///
    /// Returns an operational error when the local state root is unsafe or
    /// cannot be created privately.
    pub fn open(home: &Path) -> Result<Self, ServiceError> {
        let home = home.canonicalize().map_err(ServiceError::operational)?;
        let layout = StateLayout::new(&home);
        let pebble = home.join(".pebble");
        ensure_state_parent(&pebble)?;
        ensure_private_directory(layout.root())?;
        ensure_private_directory(&layout.root().join("repos"))?;
        Ok(Self { layout })
    }

    /// Return the versioned local state root.
    #[must_use]
    pub fn state_root(&self) -> &Path {
        self.layout.root()
    }

    fn repository_root(&self, repository: &RepositoryId) -> PathBuf {
        self.layout
            .generations(repository)
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf)
    }

    fn prepare_repository_state(&self, repository: &RepositoryId) -> Result<PathBuf, ServiceError> {
        let root = self.repository_root(repository);
        ensure_private_directory(&root)?;
        let generations = self.layout.generations(repository);
        ensure_private_directory(&generations)?;
        Ok(generations)
    }
}

/// Failure classification used by CLI and MCP adapters.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// Caller input failed bounded validation.
    #[error("invalid request: {0}")]
    Usage(String),
    /// Portable or local configuration is malformed.
    #[error("configuration error: {0}")]
    Configuration(String),
    /// A local filesystem, Git, compiler, watcher, or trace operation failed.
    #[error("operational error: {0}")]
    Operational(String),
    /// No valid immutable evidence generation is available.
    #[error("evidence unavailable: {0}")]
    EvidenceUnavailable(String),
    /// A citation no longer resolves against its exact indexed revision.
    #[error("stale evidence: {0}")]
    StaleEvidence(String),
}

impl ServiceError {
    pub(super) fn usage(error: impl std::fmt::Display) -> Self {
        Self::Usage(error.to_string())
    }

    pub(super) fn configuration(error: impl std::fmt::Display) -> Self {
        Self::Configuration(error.to_string())
    }

    pub(super) fn operational(error: impl std::fmt::Display) -> Self {
        Self::Operational(error.to_string())
    }

    pub(super) fn unavailable(error: impl std::fmt::Display) -> Self {
        Self::EvidenceUnavailable(error.to_string())
    }

    pub(super) fn stale(error: impl std::fmt::Display) -> Self {
        Self::StaleEvidence(error.to_string())
    }

    /// Return whether this failure represents unavailable or stale evidence.
    #[must_use]
    pub const fn is_evidence_failure(&self) -> bool {
        matches!(self, Self::EvidenceUnavailable(_) | Self::StaleEvidence(_))
    }
}

fn ensure_state_parent(path: &Path) -> Result<(), ServiceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            ServiceError::operational(format!("{} must be a real directory", path.display())),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(ServiceError::operational)?;
            set_private_permissions(path).map_err(ServiceError::operational)
        }
        Err(error) => Err(ServiceError::operational(error)),
    }
}

fn ensure_private_directory(path: &Path) -> Result<(), ServiceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(ServiceError::operational(format!(
                "{} must be a real directory",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(ServiceError::operational)?;
        }
        Err(error) => return Err(ServiceError::operational(error)),
    }
    set_private_permissions(path).map_err(ServiceError::operational)
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
