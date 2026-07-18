//! Durable, fail-closed publication of the active generation pointer.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use ulid::Ulid;

use super::IndexError;
use super::generation_fs::{RootBoundary, open_regular_new};

const CURRENT: &str = "CURRENT";
const CURRENT_LOCK: &str = ".CURRENT.lock";
const MAX_CURRENT_BYTES: usize = 1_024;
const MAX_TEMPORARY_ATTEMPTS: usize = 128;

pub(super) enum PreviousCurrent {
    Valid {
        bytes: Vec<u8>,
        reader: Box<super::GenerationReader>,
    },
    Absent,
}

pub(super) struct ActivationLock {
    _file: File,
}

pub(super) fn lock(root: &Path) -> Result<ActivationLock, IndexError> {
    let root_boundary = RootBoundary::inspect(root)?;
    let path = root.join(CURRENT_LOCK);
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(super::generation::no_follow_flag());
    }
    let file = options.open(&path)?;
    file.lock()?;
    let opened = file.metadata()?;
    let observed = fs::symlink_metadata(path)?;
    if !opened.file_type().is_file() || !same_identity(&opened, &observed) {
        return Err(IndexError::rebuild(
            "CURRENT activation lock identity changed",
        ));
    }
    root_boundary.verify()?;
    Ok(ActivationLock { _file: file })
}

pub(super) fn snapshot(root: &Path) -> PreviousCurrent {
    let Ok(bytes) = read(root) else {
        return PreviousCurrent::Absent;
    };
    let Ok(id) = super::generation::parse_current(&bytes) else {
        return PreviousCurrent::Absent;
    };
    let Ok(reader) = super::GenerationReader::open(root, id) else {
        return PreviousCurrent::Absent;
    };
    PreviousCurrent::Valid {
        bytes,
        reader: Box::new(reader),
    }
}

pub(super) fn publish(
    root: &Path,
    bytes: &[u8],
    previous: &PreviousCurrent,
) -> Result<(), IndexError> {
    match publish_bytes(root, bytes, true) {
        Ok(()) => Ok(()),
        Err(error) => {
            restore(root, previous)?;
            Err(error)
        }
    }
}

pub(super) fn restore(root: &Path, previous: &PreviousCurrent) -> Result<(), IndexError> {
    match previous {
        PreviousCurrent::Valid { bytes, reader } => {
            publish_bytes(root, bytes, false)?;
            if let Err(error) = reader.verify() {
                restore_absent(root)?;
                return Err(error);
            }
            Ok(())
        }
        PreviousCurrent::Absent => restore_absent(root),
    }
}

fn publish_bytes(root: &Path, bytes: &[u8], inject: bool) -> Result<(), IndexError> {
    if bytes.len() > MAX_CURRENT_BYTES {
        return Err(IndexError::rebuild("CURRENT exceeds its size limit"));
    }
    let root_boundary = RootBoundary::inspect(root)?;
    for _ in 0..MAX_TEMPORARY_ATTEMPTS {
        let temporary = root.join(format!(".CURRENT.{}.tmp", Ulid::new()));
        let mut file = match open_regular_new(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let identity = file.metadata()?;
        let publication = Publication {
            root,
            bytes,
            temporary: &temporary,
            identity: &identity,
            root_boundary: &root_boundary,
            inject,
        };
        let result = publish_file(&publication, &mut file);
        if result.is_err() {
            remove_owned(&temporary, &identity);
        }
        return result;
    }
    Err(IndexError::rebuild(
        "unable to allocate a CURRENT temporary file",
    ))
}

struct Publication<'publication> {
    root: &'publication Path,
    bytes: &'publication [u8],
    temporary: &'publication Path,
    identity: &'publication Metadata,
    root_boundary: &'publication RootBoundary,
    inject: bool,
}

fn publish_file(publication: &Publication<'_>, file: &mut File) -> Result<(), IndexError> {
    file.write_all(publication.bytes)?;
    file.flush()?;
    file.sync_all()?;
    if publication.inject {
        race(
            super::generation_races::RacePoint::CurrentTemporary,
            publication.root,
            publication.temporary,
        );
    }
    verify_named(
        publication.temporary,
        publication.identity,
        publication.bytes,
    )?;
    publication.root_boundary.verify()?;
    fs::rename(publication.temporary, publication.root.join(CURRENT))?;
    sync_directory(publication.root)?;
    if publication.inject {
        race(
            super::generation_races::RacePoint::CurrentPublished,
            publication.root,
            &publication.root.join(CURRENT),
        );
    }
    verify_named(
        &publication.root.join(CURRENT),
        publication.identity,
        publication.bytes,
    )?;
    publication.root_boundary.verify()
}

fn verify_named(path: &Path, identity: &Metadata, expected: &[u8]) -> Result<(), IndexError> {
    let observed = fs::symlink_metadata(path).map_err(IndexError::into_rebuild)?;
    if observed.file_type().is_symlink()
        || !observed.file_type().is_file()
        || !same_identity(identity, &observed)
    {
        return Err(IndexError::rebuild("CURRENT publication identity changed"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(super::generation::no_follow_flag());
    }
    let opened = options.open(path).map_err(IndexError::into_rebuild)?;
    if !same_identity(identity, &opened.metadata()?) {
        return Err(IndexError::rebuild("CURRENT publication identity changed"));
    }
    let bytes = read_bounded(opened)?;
    if bytes != expected {
        return Err(IndexError::rebuild("CURRENT publication content changed"));
    }
    Ok(())
}

pub(super) fn read(root: &Path) -> Result<Vec<u8>, IndexError> {
    let root_boundary = RootBoundary::inspect(root)?;
    let path = root.join(CURRENT);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| IndexError::rebuild(format!("CURRENT is unavailable: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(IndexError::rebuild("CURRENT is not a regular file"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(super::generation::no_follow_flag());
    }
    let file = options
        .open(path)
        .map_err(|error| IndexError::rebuild(format!("CURRENT is unavailable: {error}")))?;
    if !same_identity(&metadata, &file.metadata()?) {
        return Err(IndexError::rebuild(
            "CURRENT identity changed while opening",
        ));
    }
    let bytes = read_bounded(file)?;
    root_boundary.verify()?;
    Ok(bytes)
}

fn read_bounded(file: File) -> Result<Vec<u8>, IndexError> {
    let mut bytes = Vec::with_capacity(MAX_CURRENT_BYTES + 1);
    file.take((MAX_CURRENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CURRENT_BYTES {
        return Err(IndexError::rebuild("CURRENT exceeds its size limit"));
    }
    Ok(bytes)
}

fn restore_absent(root: &Path) -> Result<(), IndexError> {
    let current = root.join(CURRENT);
    match fs::symlink_metadata(&current) {
        Ok(metadata) => {
            let quarantine = root.join(format!(".CURRENT.{}.discard", Ulid::new()));
            fs::rename(&current, &quarantine)?;
            sync_directory(root)?;
            remove_owned(&quarantine, &metadata);
            sync_directory(root)?;
            match fs::symlink_metadata(current) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Ok(_) => Err(IndexError::rebuild("CURRENT reappeared during restoration")),
                Err(error) => Err(error.into()),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_owned(path: &Path, identity: &Metadata) {
    let Ok(observed) = fs::symlink_metadata(path) else {
        return;
    };
    if same_identity(identity, &observed) {
        let _ = fs::remove_file(path);
    }
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

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), IndexError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), IndexError> {
    Ok(())
}

#[cfg(test)]
fn race(point: super::generation_races::RacePoint, root: &Path, path: &Path) {
    super::generation_races::run(point, root, path);
}

#[cfg(not(test))]
const fn race(_point: super::generation_races::RacePoint, _root: &Path, _path: &Path) {}
