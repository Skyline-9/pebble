//! Named multi-repository workspace manifests with durable atomic storage.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::RepositoryId;

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_WORKSPACE_NAME_BYTES: usize = 128;

/// Failure while validating or persisting a workspace manifest.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    /// A filesystem operation failed.
    #[error("workspace I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// A workspace name was empty, unsafe, or too long.
    #[error("invalid workspace name: {0}")]
    InvalidName(String),
    /// A workspace with this name already has a manifest.
    #[error("workspace {0} already exists")]
    AlreadyExists(String),
    /// No manifest exists for this workspace name.
    #[error("workspace {0} was not found")]
    NotFound(String),
    /// The manifest file was malformed or unsafe to trust.
    #[error("invalid workspace manifest: {0}")]
    InvalidManifest(String),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    schema: u32,
    repositories: Vec<RepositoryId>,
}

/// A named list of registered repositories, persisted as durable JSON.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceManifest {
    state_root: PathBuf,
    name: String,
    repositories: Vec<RepositoryId>,
}

impl WorkspaceManifest {
    /// Create a new empty workspace manifest under `state_root`.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe name, an already-existing workspace,
    /// or an I/O failure.
    pub fn create(state_root: &Path, name: &str) -> Result<Self, WorkspaceError> {
        let name = validate_workspace_name(name)?;
        fs::create_dir_all(workspaces_dir(state_root))?;
        if manifest_path(state_root, &name).exists() {
            return Err(WorkspaceError::AlreadyExists(name));
        }
        let manifest = Self {
            state_root: state_root.to_path_buf(),
            name,
            repositories: Vec::new(),
        };
        manifest.store()?;
        Ok(manifest)
    }

    /// Load an existing workspace manifest from under `state_root`.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe name, a missing workspace, malformed
    /// or oversized manifest data, or an I/O failure.
    pub fn load(state_root: &Path, name: &str) -> Result<Self, WorkspaceError> {
        let name = validate_workspace_name(name)?;
        let path = manifest_path(state_root, &name);
        let bytes = match read_bounded(&path) {
            Ok(bytes) => bytes,
            Err(error) if is_not_found(&error) => return Err(WorkspaceError::NotFound(name)),
            Err(error) => return Err(error),
        };
        let file: ManifestFile = serde_json::from_slice(&bytes)
            .map_err(|error| WorkspaceError::InvalidManifest(error.to_string()))?;
        if file.schema != 1 {
            return Err(WorkspaceError::InvalidManifest(
                "workspace schema must equal 1".to_owned(),
            ));
        }
        Ok(Self {
            state_root: state_root.to_path_buf(),
            name,
            repositories: file.repositories,
        })
    }

    /// List the names of every workspace manifest under `state_root`.
    ///
    /// # Errors
    ///
    /// Returns an error for an I/O failure.
    pub fn list(state_root: &Path) -> Result<Vec<String>, WorkspaceError> {
        let entries = match fs::read_dir(workspaces_dir(state_root)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let file_name = entry.file_name();
            if let Some(name) = file_name.to_string_lossy().strip_suffix(".json") {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Add a repository to this workspace and persist the change.
    ///
    /// Adding a repository already present in the workspace is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error for an I/O failure while persisting the manifest.
    pub fn add_repository(&mut self, repository: RepositoryId) -> Result<(), WorkspaceError> {
        if !self.repositories.contains(&repository) {
            self.repositories.push(repository);
            self.repositories.sort();
        }
        self.store()
    }

    /// Remove a repository from this workspace and persist the change.
    ///
    /// Removing a repository absent from the workspace is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error for an I/O failure while persisting the manifest.
    pub fn remove_repository(&mut self, repository: &RepositoryId) -> Result<(), WorkspaceError> {
        self.repositories.retain(|entry| entry != repository);
        self.store()
    }

    /// Return this workspace's validated name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the repositories currently listed by this workspace.
    #[must_use]
    pub fn repositories(&self) -> &[RepositoryId] {
        &self.repositories
    }

    fn store(&self) -> Result<(), WorkspaceError> {
        let directory = workspaces_dir(&self.state_root);
        fs::create_dir_all(&directory)?;
        let target = manifest_path(&self.state_root, &self.name);
        let temporary = directory.join(format!(
            ".{}-{}-{}.tmp",
            self.name,
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = serde_json::to_vec_pretty(&ManifestFile {
            schema: 1,
            repositories: self.repositories.clone(),
        })
        .map_err(|error| WorkspaceError::InvalidManifest(error.to_string()))?;
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.flush()?;
            file.sync_all()?;
            fs::rename(&temporary, &target)?;
            sync_directory(&directory)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn workspaces_dir(state_root: &Path) -> PathBuf {
    state_root.join("workspaces")
}

fn manifest_path(state_root: &Path, name: &str) -> PathBuf {
    workspaces_dir(state_root).join(format!("{name}.json"))
}

fn validate_workspace_name(name: &str) -> Result<String, WorkspaceError> {
    if name.is_empty()
        || name.len() > MAX_WORKSPACE_NAME_BYTES
        || matches!(name, "." | "..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(WorkspaceError::InvalidName(name.to_owned()));
    }
    Ok(name.to_owned())
}

fn is_not_found(error: &WorkspaceError) -> bool {
    matches!(error, WorkspaceError::Io(io_error) if io_error.kind() == std::io::ErrorKind::NotFound)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, WorkspaceError> {
    let mut options = OpenOptions::new();
    options.read(true);
    set_no_follow(&mut options);
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(unsafe_manifest());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    Read::by_ref(&mut file)
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_MANIFEST_BYTES {
        return Err(unsafe_manifest());
    }
    Ok(bytes)
}

fn unsafe_manifest() -> WorkspaceError {
    WorkspaceError::InvalidManifest(
        "manifest must be a regular file no larger than 1 MiB".to_owned(),
    )
}

#[cfg(unix)]
fn set_no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(no_follow_flag());
}

#[cfg(not(unix))]
const fn set_no_follow(_options: &mut OpenOptions) {}

#[cfg(any(target_os = "linux", target_os = "android"))]
const fn no_follow_flag() -> i32 {
    0x2_0000
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
const fn no_follow_flag() -> i32 {
    0x100
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), WorkspaceError> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), WorkspaceError> {
    Ok(())
}
