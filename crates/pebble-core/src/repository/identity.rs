//! Canonical repository and dirty-snapshot identity derivation.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::RepositoryId;

use super::RepositoryError;

static NEXT_ULID: AtomicU64 = AtomicU64::new(0);

pub(super) fn identity_for_remote(remote: Option<&str>) -> Result<RepositoryId, RepositoryError> {
    match remote {
        Some(remote) => RepositoryId::try_from(normalize_remote(remote)?).map_err(Into::into),
        None => RepositoryId::try_from(new_ulid()?).map_err(Into::into),
    }
}

fn new_ulid() -> Result<String, RepositoryError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RepositoryError::InvalidConfig(error.to_string()))?;
    let timestamp = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    let counter = NEXT_ULID.fetch_add(1, Ordering::Relaxed);
    let mut hasher = blake3::Hasher::new();
    hasher.update(&elapsed.as_nanos().to_le_bytes());
    hasher.update(&std::process::id().to_le_bytes());
    hasher.update(&counter.to_le_bytes());
    let random = hasher.finalize();
    let mut value = u128::from(timestamp & 0xFFFF_FFFF_FFFF) << 80;
    for (index, byte) in random.as_bytes()[..10].iter().enumerate() {
        value |= u128::from(*byte) << (72 - 8 * index);
    }
    let alphabet = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut encoded = [b'0'; 26];
    for character in encoded.iter_mut().rev() {
        *character = alphabet[(value & 31) as usize];
        value >>= 5;
    }
    Ok(encoded.into_iter().map(char::from).collect())
}

fn normalize_remote(remote: &str) -> Result<String, RepositoryError> {
    let remote = remote.trim().trim_end_matches('/');
    let (host, path) = if let Some((_, location)) = remote.split_once("://") {
        let location = location
            .rsplit_once('@')
            .map_or(location, |(_, without_user)| without_user);
        location.split_once('/').ok_or_else(|| invalid(remote))?
    } else {
        let location = remote
            .rsplit_once('@')
            .map_or(remote, |(_, without_user)| without_user);
        location.split_once(':').ok_or_else(|| invalid(remote))?
    };
    let host = host.to_ascii_lowercase();
    let path = path
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path);
    if host.is_empty() || path.is_empty() || path.split('/').any(invalid_component) {
        return Err(invalid(remote));
    }
    if !host.bytes().all(portable) || !path.bytes().all(|byte| portable(byte) || byte == b'/') {
        return Err(invalid(remote));
    }
    let mut identity = String::from("git");
    for component in std::iter::once(host.as_str()).chain(path.split('/')) {
        identity.push('.');
        identity.push_str(&component.len().to_string());
        identity.push('.');
        identity.push_str(component);
    }
    Ok(identity)
}

fn invalid_component(component: &str) -> bool {
    component.is_empty() || matches!(component, "." | "..")
}

const fn portable(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

fn invalid(remote: &str) -> RepositoryError {
    RepositoryError::InvalidRemote(remote.to_owned())
}

pub(super) fn hash_worktree(
    hasher: &mut blake3::Hasher,
    repository: &Path,
    relative: &str,
) -> Result<(), RepositoryError> {
    validate_git_path(relative)?;
    let root = repository.canonicalize()?;
    if !verify_ancestors(repository, &root, relative)? {
        hash_worktree_entry(hasher, b"missing", b"");
        return Ok(());
    }
    let path = repository.join(relative);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        let _ = verify_ancestors(repository, &root, relative)?;
        hash_worktree_entry(hasher, b"missing", b"");
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&path)?;
        verify_unchanged(repository, &root, relative, &path, &metadata)?;
        hash_worktree_entry(
            hasher,
            b"symbolic-link",
            target.as_os_str().as_encoded_bytes(),
        );
        return Ok(());
    }
    if !metadata.is_file() {
        verify_unchanged(repository, &root, relative, &path, &metadata)?;
        hash_worktree_entry(hasher, b"non-file", b"");
        return Ok(());
    }
    ensure_contained(&root, &path.canonicalize()?)?;
    let mut file = fs::File::open(&path)?;
    let opened = file.metadata()?;
    if !same_file(&metadata, &opened) {
        return Err(changed_path());
    }
    let mut content = blake3::Hasher::new();
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        content.update(&buffer[..count]);
    }
    ensure_contained(&root, &path.canonicalize()?)?;
    verify_unchanged(repository, &root, relative, &path, &opened)?;
    hash_worktree_entry(hasher, b"regular-file", content.finalize().as_bytes());
    Ok(())
}

fn hash_worktree_entry(hasher: &mut blake3::Hasher, kind: &[u8], value: &[u8]) {
    hash_part(hasher, kind);
    hash_part(hasher, value);
}

fn verify_ancestors(
    repository: &Path,
    root: &Path,
    relative: &str,
) -> Result<bool, RepositoryError> {
    if repository.canonicalize()? != root {
        return Err(changed_path());
    }
    let mut current = PathBuf::from(repository);
    let mut components = Path::new(relative).components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break;
        }
        current.push(component);
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            return Ok(false);
        };
        if metadata.file_type().is_symlink() {
            return Err(RepositoryError::InvalidGitOutput {
                operation: "path",
                message: "symlinked ancestor".to_owned(),
            });
        }
        if !metadata.is_dir() {
            return Err(RepositoryError::InvalidGitOutput {
                operation: "path",
                message: "non-directory ancestor".to_owned(),
            });
        }
        ensure_contained(root, &current.canonicalize()?)?;
    }
    Ok(true)
}

fn verify_unchanged(
    repository: &Path,
    root: &Path,
    relative: &str,
    path: &Path,
    expected: &fs::Metadata,
) -> Result<(), RepositoryError> {
    if !verify_ancestors(repository, root, relative)? {
        return Err(changed_path());
    }
    let observed = fs::symlink_metadata(path)?;
    if same_file(expected, &observed) {
        Ok(())
    } else {
        Err(changed_path())
    }
}

fn ensure_contained(root: &Path, path: &Path) -> Result<(), RepositoryError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(RepositoryError::InvalidGitOutput {
            operation: "path",
            message: "path escaped repository".to_owned(),
        })
    }
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.file_type() == right.file_type()
        && (!left.is_file()
            || (left.len() == right.len()
                && left.mtime() == right.mtime()
                && left.mtime_nsec() == right.mtime_nsec()
                && left.ctime() == right.ctime()
                && left.ctime_nsec() == right.ctime_nsec()))
}

#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
        && left.file_type() == right.file_type()
}

fn changed_path() -> RepositoryError {
    RepositoryError::InvalidGitOutput {
        operation: "path",
        message: "path changed during fingerprinting".to_owned(),
    }
}

pub(super) fn validate_git_path(path: &str) -> Result<(), RepositoryError> {
    let bytes = path.as_bytes();
    if path.is_empty()
        || path.contains('\\')
        || Path::new(path).is_absolute()
        || (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1..3] == *b":/")
        || Path::new(path).components().any(|item| {
            matches!(
                item,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(RepositoryError::InvalidGitOutput {
            operation: "path",
            message: "malformed record".to_owned(),
        });
    }
    Ok(())
}

pub(super) fn hash_part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::same_file;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn same_file_rejects_in_place_mutation() -> std::io::Result<()> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("pebble-same-file-{}-{suffix}", std::process::id()));
        fs::write(&path, b"before")?;
        let before = fs::metadata(&path)?;
        fs::write(&path, b"after mutation")?;
        let after = fs::metadata(&path)?;

        assert!(!same_file(&before, &after));
        fs::remove_file(path)
    }
}
