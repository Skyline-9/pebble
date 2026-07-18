use std::path::{Path, PathBuf};

use super::{WatchError, WatchResult};

pub(super) fn validate(repository: &Path, generations: &Path) -> WatchResult<(PathBuf, PathBuf)> {
    let repository = real_directory(repository, "repository root")?;
    let generations = real_directory(generations, "generation root")?;
    if repository.starts_with(&generations) || generations.starts_with(&repository) {
        return Err(WatchError::invalid_path(
            "repository and generation roots must be separate",
        ));
    }
    Ok((repository, generations))
}

fn real_directory(path: &Path, label: &str) -> WatchResult<PathBuf> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| WatchError::invalid_path(format!("{label}: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WatchError::invalid_path(format!(
            "{label} must be a real directory"
        )));
    }
    path.canonicalize()
        .map_err(|error| WatchError::invalid_path(format!("{label}: {error}")))
}
