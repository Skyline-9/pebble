//! Atomic local checkout registry.

use std::fs::{self, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::domain::RepositoryId;

use super::RepositoryError;

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);
const MAX_REGISTRY_BYTES: u64 = 1024 * 1024;

#[cfg(test)]
mod tests;

/// One canonical repository checkout registered on this machine.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisteredRepository {
    repository_id: RepositoryId,
    checkout: PathBuf,
    alternate_worktree: bool,
}

impl RegisteredRepository {
    /// Return the canonical repository identity.
    #[must_use]
    pub const fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Return the canonical local checkout path.
    #[must_use]
    pub fn checkout(&self) -> &Path {
        &self.checkout
    }

    /// Return whether this registration explicitly permits another checkout.
    #[must_use]
    pub const fn alternate_worktree(&self) -> bool {
        self.alternate_worktree
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    schema: u32,
    registrations: Vec<RegisteredRepository>,
}

/// Persistent registry stored as atomic JSON under the versioned state root.
#[derive(Clone, Debug)]
pub struct RepositoryRegistry {
    state_root: PathBuf,
}

impl RepositoryRegistry {
    /// Create a registry rooted at `state_root`.
    #[must_use]
    pub fn new(state_root: &Path) -> Self {
        Self {
            state_root: state_root.to_path_buf(),
        }
    }

    /// Register a checkout for a canonical repository identity.
    ///
    /// A different checkout using an existing identity is rejected unless the
    /// new registration is explicitly marked as an alternate worktree.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths, duplicate checkouts without
    /// alternate-worktree consent, malformed registry data, or I/O failures.
    pub fn register(
        &self,
        repository_id: &RepositoryId,
        checkout: &Path,
        alternate_worktree: bool,
    ) -> Result<(), RepositoryError> {
        fs::create_dir_all(&self.state_root)?;
        let checkout = checkout.canonicalize()?;
        let _lock = self.lock_exclusive()?;
        let mut file = self.load_file()?;
        if file
            .registrations
            .iter()
            .any(|entry| entry.repository_id == *repository_id && entry.checkout == checkout)
        {
            return Ok(());
        }
        if !alternate_worktree
            && let Some(existing) = file
                .registrations
                .iter()
                .find(|entry| entry.repository_id == *repository_id)
        {
            return Err(RepositoryError::DuplicateCheckout {
                repository: repository_id.to_string(),
                existing: existing.checkout.clone(),
            });
        }
        file.schema = 1;
        file.registrations.push(RegisteredRepository {
            repository_id: repository_id.clone(),
            checkout,
            alternate_worktree,
        });
        file.registrations.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then_with(|| left.checkout.cmp(&right.checkout))
        });
        self.store(&file)
    }

    /// Load all registrations in deterministic identity and path order.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed data, unsafe files, files larger than
    /// 1 MiB, concurrent replacement, or I/O failures.
    pub fn registrations(&self) -> Result<Vec<RegisteredRepository>, RepositoryError> {
        Ok(self.load_file()?.registrations)
    }

    fn load_file(&self) -> Result<RegistryFile, RepositoryError> {
        let path = self.state_root.join("registry.json");
        let mut options = OpenOptions::new();
        options.read(true);
        set_no_follow(&mut options);
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(empty_file()),
            Err(error) => return Err(error.into()),
        };
        let opened = file.metadata()?;
        if !opened.is_file() || opened.len() > MAX_REGISTRY_BYTES {
            return Err(unsafe_registry());
        }
        let path_metadata = fs::symlink_metadata(&path)?;
        if !same_file(&opened, &path_metadata) {
            return Err(unsafe_registry());
        }
        super::registry_race::run(&path);
        let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
        Read::by_ref(&mut file)
            .take(MAX_REGISTRY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REGISTRY_BYTES {
            return Err(unsafe_registry());
        }
        let after = file.metadata()?;
        let path_after = fs::symlink_metadata(&path)?;
        if !same_file(&opened, &after) || !same_file(&opened, &path_after) {
            return Err(unsafe_registry());
        }
        let file: RegistryFile = serde_json::from_slice(&bytes)
            .map_err(|error| RepositoryError::InvalidConfig(error.to_string()))?;
        if file.schema != 1 {
            return Err(RepositoryError::InvalidConfig(
                "registry schema must equal 1".to_owned(),
            ));
        }
        Ok(file)
    }

    fn lock_exclusive(&self) -> Result<fs::File, RepositoryError> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.state_root.join(".registry.lock"))?;
        lock.lock()?;
        Ok(lock)
    }

    fn store(&self, registry: &RegistryFile) -> Result<(), RepositoryError> {
        let target = self.state_root.join("registry.json");
        let temporary = self.state_root.join(format!(
            ".registry-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = serde_json::to_vec_pretty(registry)
            .map_err(|error| RepositoryError::InvalidConfig(error.to_string()))?;
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.flush()?;
            file.sync_all()?;
            fs::rename(&temporary, target)?;
            sync_directory(&self.state_root)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

const fn empty_file() -> RegistryFile {
    RegistryFile {
        schema: 1,
        registrations: Vec::new(),
    }
}

fn unsafe_registry() -> RepositoryError {
    RepositoryError::InvalidConfig(
        "registry must be an unchanged regular file no larger than 1 MiB".to_owned(),
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

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), RepositoryError> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), RepositoryError> {
    Ok(())
}
