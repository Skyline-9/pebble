//! Symlink-resistant repository configuration filesystem boundary.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::RepositoryError;

pub(super) fn read(repository: &Path, limit: u64) -> Result<Vec<u8>, RepositoryError> {
    let boundary = Boundary::existing(repository)?;
    let path = boundary.directory.join("pebble.toml");
    let before = regular_metadata(&path)?;
    ensure_contained(&boundary.root, &path.canonicalize()?)?;
    let mut file = File::open(&path)?;
    let opened = file.metadata()?;
    if !stable(&before, &opened) {
        return Err(invalid_boundary());
    }
    if opened.len() > limit {
        return Err(RepositoryError::InvalidConfig(
            "file exceeds 1 MiB".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    boundary.verify()?;
    let after = regular_metadata(&path)?;
    ensure_contained(&boundary.root, &path.canonicalize()?)?;
    if !stable(&opened, &after) {
        return Err(invalid_boundary());
    }
    Ok(bytes)
}

pub(super) fn exists(repository: &Path) -> Result<bool, RepositoryError> {
    let Some(boundary) = Boundary::optional(repository)? else {
        return Ok(false);
    };
    let path = boundary.directory.join("pebble.toml");
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(invalid_boundary());
            }
            ensure_contained(&boundary.root, &path.canonicalize()?)?;
            boundary.verify()?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            boundary.verify()?;
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn create(repository: &Path, contents: &[u8]) -> Result<bool, RepositoryError> {
    let boundary = Boundary::create(repository)?;
    let path = boundary.directory.join("pebble.toml");
    let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let opened = file.metadata()?;
    let observed = regular_metadata(&path)?;
    ensure_contained(&boundary.root, &path.canonicalize()?)?;
    boundary.verify()?;
    if !stable(&opened, &observed) {
        return Err(invalid_boundary());
    }
    file.write_all(contents)?;
    file.flush()?;
    file.sync_all()?;
    boundary.verify()?;
    let after = regular_metadata(&path)?;
    ensure_contained(&boundary.root, &path.canonicalize()?)?;
    if !stable(&file.metadata()?, &after) {
        return Err(invalid_boundary());
    }
    sync_directory(&boundary.directory)?;
    Ok(true)
}

struct Boundary {
    root: PathBuf,
    directory: PathBuf,
    metadata: Metadata,
}

impl Boundary {
    fn existing(repository: &Path) -> Result<Self, RepositoryError> {
        Self::optional(repository)?.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "configuration directory").into()
        })
    }

    fn optional(repository: &Path) -> Result<Option<Self>, RepositoryError> {
        let root = repository.canonicalize()?;
        let directory = repository.join(".pebble");
        let metadata = match fs::symlink_metadata(&directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        Self::validated(root, directory, metadata).map(Some)
    }

    fn create(repository: &Path) -> Result<Self, RepositoryError> {
        if let Some(boundary) = Self::optional(repository)? {
            return Ok(boundary);
        }
        let directory = repository.join(".pebble");
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let root = repository.canonicalize()?;
        let metadata = fs::symlink_metadata(&directory)?;
        Self::validated(root, directory, metadata)
    }

    fn validated(
        root: PathBuf,
        directory: PathBuf,
        metadata: Metadata,
    ) -> Result<Self, RepositoryError> {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid_boundary());
        }
        ensure_contained(&root, &directory.canonicalize()?)?;
        let boundary = Self {
            root,
            directory,
            metadata,
        };
        boundary.verify()?;
        Ok(boundary)
    }

    fn verify(&self) -> Result<(), RepositoryError> {
        let observed = fs::symlink_metadata(&self.directory)?;
        if observed.file_type().is_symlink()
            || !observed.is_dir()
            || !same_identity(&self.metadata, &observed)
        {
            return Err(invalid_boundary());
        }
        ensure_contained(&self.root, &self.directory.canonicalize()?)
    }
}

fn regular_metadata(path: &Path) -> Result<Metadata, RepositoryError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_boundary());
    }
    Ok(metadata)
}

fn ensure_contained(root: &Path, path: &Path) -> Result<(), RepositoryError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(invalid_boundary())
    }
}

#[cfg(unix)]
fn stable(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.file_type() == right.file_type()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

#[cfg(unix)]
fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino() && left.file_type() == right.file_type()
}

#[cfg(not(unix))]
fn stable(left: &Metadata, right: &Metadata) -> bool {
    left.file_type() == right.file_type()
        && left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
}

#[cfg(not(unix))]
fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    left.file_type() == right.file_type()
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), RepositoryError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), RepositoryError> {
    Ok(())
}

fn invalid_boundary() -> RepositoryError {
    RepositoryError::InvalidConfig(
        "configuration boundary changed or contains a symlink".to_owned(),
    )
}
