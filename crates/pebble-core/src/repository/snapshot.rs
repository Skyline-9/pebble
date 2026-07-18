//! Bounded, one-file-at-a-time repository snapshots.

use std::collections::VecDeque;
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::domain::{FileId, RepositoryId, WorktreeRevision};

use super::traversal::{slash_path, walk};
use super::{RepositoryConfig, RepositoryError, SystemGit};

const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SOURCE_CAPACITY: usize = 32 * 1024 * 1024;

/// A text source file loaded from one repository snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    id: FileId,
    path: String,
    content_hash: String,
    contents: String,
}

impl SourceFile {
    /// Return the deterministic repository-and-path identity.
    #[must_use]
    pub const fn id(&self) -> &FileId {
        &self.id
    }

    /// Return the normalized repository-relative slash path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the BLAKE3 digest of the exact source bytes.
    #[must_use]
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    /// Return the validated UTF-8 source text.
    #[must_use]
    pub fn contents(&self) -> &str {
        &self.contents
    }
}

/// Nonfatal reason that a traversed path was not emitted as source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkipReason {
    /// Symbolic links are never followed.
    SymbolicLink,
    /// The file exceeded the 32 MiB source limit.
    TooLarge,
    /// A NUL byte identified binary input.
    Binary,
    /// Source bytes were not valid UTF-8.
    InvalidUtf8,
}

/// A deterministic file-scoped nonfatal traversal diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkipDiagnostic {
    path: String,
    reason: SkipReason,
}

impl SkipDiagnostic {
    pub(crate) const fn new(path: String, reason: SkipReason) -> Self {
        Self { path, reason }
    }

    /// Return the normalized repository-relative path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return why the path was skipped.
    #[must_use]
    pub const fn reason(&self) -> SkipReason {
        self.reason
    }
}

/// Deterministic iterator that loads at most one bounded source per step.
///
/// Callers must exhaust the iterator and accept every returned `Result`.
/// Reaching the end rechecks the worktree revision and reports
/// [`RepositoryError::WorktreeChanged`] if any source changed after opening.
pub struct RepositorySnapshot {
    root: PathBuf,
    repository_id: RepositoryId,
    revision: WorktreeRevision,
    git: SystemGit,
    paths: VecDeque<PathBuf>,
    diagnostics: Vec<SkipDiagnostic>,
    complete: bool,
}

impl RepositorySnapshot {
    /// Capture revision metadata and prepare a path-only repository traversal.
    ///
    /// # Errors
    ///
    /// Returns an error for Git failures, inaccessible paths, invalid path
    /// encodings, or repository escapes.
    pub fn open(
        repository: &Path,
        config: &RepositoryConfig,
        git: &SystemGit,
    ) -> Result<Self, RepositoryError> {
        let root_metadata = fs::symlink_metadata(repository)?;
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(RepositoryError::Traversal(
                "repository root must be a real directory".to_owned(),
            ));
        }
        let root = repository.canonicalize()?;
        let revision = git.revision(&root)?;
        let traversal = walk(&root, config)?;
        Ok(Self {
            revision,
            root,
            repository_id: config.repository_id().clone(),
            git: git.clone(),
            paths: traversal.paths.into(),
            diagnostics: traversal.diagnostics,
            complete: false,
        })
    }

    /// Return the Git worktree revision captured before traversal began.
    ///
    /// This revision describes the emitted sources only after the iterator is
    /// exhausted without an error.
    #[must_use]
    pub const fn revision(&self) -> &WorktreeRevision {
        &self.revision
    }

    /// Return nonfatal diagnostics accumulated through the current position.
    #[must_use]
    pub fn diagnostics(&self) -> &[SkipDiagnostic] {
        &self.diagnostics
    }
}

impl Iterator for RepositorySnapshot {
    type Item = Result<SourceFile, RepositoryError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.complete {
            return None;
        }
        loop {
            let Some(relative) = self.paths.pop_front() else {
                self.complete = true;
                return match self.git.revision(&self.root) {
                    Ok(revision) if revision == self.revision => None,
                    Ok(_) => Some(Err(RepositoryError::WorktreeChanged)),
                    Err(error) => Some(Err(error)),
                };
            };
            let path = match slash_path(&relative) {
                Ok(path) => path,
                Err(error) => return Some(Err(error)),
            };
            match load(&self.root, &relative, &path, &self.repository_id) {
                Ok(Load::Source(source)) => return Some(Ok(source)),
                Ok(Load::Skipped(reason)) => {
                    self.diagnostics.push(SkipDiagnostic::new(path, reason));
                }
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

enum Load {
    Source(SourceFile),
    Skipped(SkipReason),
}

fn load(
    root: &Path,
    relative: &Path,
    path: &str,
    repository_id: &RepositoryId,
) -> Result<Load, RepositoryError> {
    let candidate = root.join(relative);
    let before = fs::symlink_metadata(&candidate)?;
    if before.file_type().is_symlink() {
        return Ok(Load::Skipped(SkipReason::SymbolicLink));
    }
    if before.len() > MAX_SOURCE_BYTES {
        return Ok(Load::Skipped(SkipReason::TooLarge));
    }
    ensure_contained(root, &candidate)?;
    let mut file = File::open(&candidate)?;
    let opened = file.metadata()?;
    if !opened.is_file() || !same_file(&before, &opened) {
        return Err(changed(path));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    let mut hasher = blake3::Hasher::new();
    let mut chunk = [0; 8192];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_SOURCE_CAPACITY {
            return Ok(Load::Skipped(SkipReason::TooLarge));
        }
        hasher.update(&chunk[..count]);
        bytes.extend_from_slice(&chunk[..count]);
    }
    let after = file.metadata()?;
    ensure_contained(root, &candidate)?;
    if !same_file(&opened, &after) {
        return Err(changed(path));
    }
    if bytes.contains(&0) {
        return Ok(Load::Skipped(SkipReason::Binary));
    }
    let contents = String::from_utf8(bytes)
        .map_err(|_| SkipReason::InvalidUtf8)
        .map_err(Load::Skipped);
    match contents {
        Ok(contents) => Ok(Load::Source(SourceFile {
            id: FileId::derive(repository_id, path),
            path: path.to_owned(),
            content_hash: hasher.finalize().to_hex().to_string(),
            contents,
        })),
        Err(skipped) => Ok(skipped),
    }
}

fn ensure_contained(root: &Path, path: &Path) -> Result<(), RepositoryError> {
    let canonical = path.canonicalize()?;
    if canonical.starts_with(root) {
        Ok(())
    } else {
        Err(RepositoryError::Traversal(
            "path escaped repository".to_owned(),
        ))
    }
}

fn changed(path: &str) -> RepositoryError {
    RepositoryError::Traversal(format!("file changed while reading: {path}"))
}

#[cfg(unix)]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.file_type() == right.file_type()
        && left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

#[cfg(not(unix))]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    left.file_type() == right.file_type()
        && left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
}
