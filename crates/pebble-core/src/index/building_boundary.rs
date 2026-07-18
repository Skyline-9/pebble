//! Identity pins for mutable generation creation paths.

use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

use super::IndexError;

pub(super) struct BuildingBoundary {
    root: PathBuf,
    root_canonical: PathBuf,
    root_metadata: Metadata,
    directory: PathBuf,
    directory_canonical: PathBuf,
    directory_metadata: Metadata,
}

impl BuildingBoundary {
    pub(super) fn inspect(root: &Path, directory: &Path) -> Result<Self, IndexError> {
        let root_metadata = real_directory(root, "generation root is invalid")?;
        let root_canonical = canonical(root)?;
        let directory_metadata =
            real_directory(directory, "building generation is not a real directory")?;
        let directory_canonical = canonical(directory)?;
        if directory_canonical.parent() != Some(root_canonical.as_path()) {
            return Err(invalid("building generation escapes its root"));
        }
        Ok(Self {
            root: root.to_owned(),
            root_canonical,
            root_metadata,
            directory: directory.to_owned(),
            directory_canonical,
            directory_metadata,
        })
    }

    pub(super) fn verify(&self) -> Result<(), IndexError> {
        let root_metadata = real_directory(&self.root, "generation root changed")?;
        let directory_metadata = real_directory(&self.directory, "building generation changed")?;
        if canonical(&self.root)? != self.root_canonical
            || canonical(&self.directory)? != self.directory_canonical
            || !same_identity(&self.root_metadata, &root_metadata)
            || !same_identity(&self.directory_metadata, &directory_metadata)
        {
            return Err(invalid("building generation identity changed"));
        }
        Ok(())
    }

    pub(super) fn pin_lexical(&self, path: &Path) -> Result<LexicalBoundary, IndexError> {
        self.verify()?;
        let metadata = real_directory(path, "building lexical index is not a real directory")?;
        let canonical = canonical(path)?;
        if canonical.parent() != Some(self.directory_canonical.as_path()) {
            return Err(invalid("building lexical index escapes its generation"));
        }
        Ok(LexicalBoundary {
            path: path.to_owned(),
            canonical,
            directory: super::pinned_directory::PinnedDirectory::open_expected(path, &metadata)?,
            metadata,
        })
    }
}

pub(super) struct LexicalBoundary {
    path: PathBuf,
    canonical: PathBuf,
    metadata: Metadata,
    directory: super::pinned_directory::PinnedDirectory,
}

impl LexicalBoundary {
    pub(super) fn verify(&self) -> Result<(), IndexError> {
        let metadata = real_directory(&self.path, "building lexical index changed")?;
        if canonical(&self.path)? != self.canonical || !same_identity(&self.metadata, &metadata) {
            return Err(invalid("building lexical index identity changed"));
        }
        Ok(())
    }

    pub(super) fn directory(&self) -> super::pinned_directory::PinnedDirectory {
        self.directory.clone()
    }
}

fn real_directory(path: &Path, message: &'static str) -> Result<Metadata, IndexError> {
    let metadata = fs::symlink_metadata(path).map_err(IndexError::into_rebuild)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid(message));
    }
    Ok(metadata)
}

fn canonical(path: &Path) -> Result<PathBuf, IndexError> {
    path.canonicalize().map_err(IndexError::into_rebuild)
}

fn invalid(message: &'static str) -> IndexError {
    IndexError::rebuild(message)
}

#[cfg(unix)]
fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino() && left.file_type() == right.file_type()
}

#[cfg(not(unix))]
fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    left.file_type() == right.file_type()
        && left.created().ok() == right.created().ok()
        && left.modified().ok() == right.modified().ok()
}
