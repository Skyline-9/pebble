//! Platform support for the lexical directory capability.

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use super::IndexError;

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn descriptor_alias(
    file: &File,
    expected: &Metadata,
    _path: &Path,
) -> Result<PathBuf, IndexError> {
    use std::os::fd::AsRawFd;

    checked_alias(
        PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())),
        expected,
    )
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(super) fn descriptor_alias(
    _file: &File,
    expected: &Metadata,
    path: &Path,
) -> Result<PathBuf, IndexError> {
    let canonical = path.canonicalize().map_err(IndexError::into_rebuild)?;
    checked_alias(canonical, expected)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
pub(super) fn descriptor_alias(
    _file: &File,
    _expected: &Metadata,
    _path: &Path,
) -> Result<PathBuf, IndexError> {
    Err(IndexError::rebuild(
        "lexical directory capabilities are unavailable on this platform",
    ))
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
fn checked_alias(alias: PathBuf, expected: &Metadata) -> Result<PathBuf, IndexError> {
    let metadata = alias.metadata().map_err(|error| {
        IndexError::rebuild(format!(
            "lexical directory capability path is unavailable: {error}"
        ))
    })?;
    if !metadata.is_dir() || !same_identity(expected, &metadata) {
        return Err(IndexError::rebuild(
            "lexical directory capability path does not preserve directory identity",
        ));
    }
    let descendant = alias.join(".").metadata().map_err(|error| {
        IndexError::rebuild(format!(
            "lexical directory capability does not support descendant I/O: {error}"
        ))
    })?;
    if !same_identity(expected, &descendant) {
        return Err(IndexError::rebuild(
            "lexical directory capability descendant changed identity",
        ));
    }
    Ok(alias)
}

#[cfg(unix)]
pub(super) fn no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;

    options.custom_flags(super::generation::no_follow_flag());
}

#[cfg(not(unix))]
pub(super) fn no_follow(_options: &mut OpenOptions) {}

#[cfg(unix)]
pub(super) fn read_exact_at(file: &File, bytes: &mut [u8], offset: usize) -> io::Result<()> {
    use std::os::unix::fs::FileExt;

    file.read_exact_at(bytes, offset as u64)
}

#[cfg(not(unix))]
pub(super) fn read_exact_at(file: &File, bytes: &mut [u8], offset: usize) -> io::Result<()> {
    use std::io::{Read, Seek};

    let mut file = file.try_clone()?;
    file.seek(io::SeekFrom::Start(offset as u64))?;
    file.read_exact(bytes)
}

#[cfg(unix)]
pub(super) fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino() && left.file_type() == right.file_type()
}

#[cfg(not(unix))]
pub(super) fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    left.file_type() == right.file_type()
        && left.created().ok() == right.created().ok()
        && left.modified().ok() == right.modified().ok()
}
