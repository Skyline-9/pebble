//! Deterministic path-only repository traversal.

use std::path::{Component, Path, PathBuf};

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use super::{RepositoryConfig, RepositoryError, SkipDiagnostic, SkipReason};

pub struct Traversal {
    pub paths: Vec<PathBuf>,
    pub diagnostics: Vec<SkipDiagnostic>,
}

pub fn walk(repository: &Path, config: &RepositoryConfig) -> Result<Traversal, RepositoryError> {
    let include = matcher(config.include())?;
    let exclude = matcher(config.exclude())?;
    let mut builder = WalkBuilder::new(repository);
    builder
        .hidden(false)
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .sort_by_file_name(std::ffi::OsStr::cmp);
    let mut paths = Vec::new();
    let mut diagnostics = Vec::new();
    for result in builder.build() {
        let entry = result.map_err(|error| RepositoryError::Traversal(error.to_string()))?;
        let relative = entry
            .path()
            .strip_prefix(repository)
            .map_err(|error| RepositoryError::Traversal(error.to_string()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let path = slash_path(relative)?;
        if internal(&path) {
            continue;
        }
        let file_type = entry
            .file_type()
            .ok_or_else(|| RepositoryError::Traversal(format!("unknown file type: {path}")))?;
        if file_type.is_symlink() {
            diagnostics.push(SkipDiagnostic::new(path, SkipReason::SymbolicLink));
        } else if file_type.is_file()
            && include
                .matched_path_or_any_parents(relative, false)
                .is_ignore()
            && !exclude
                .matched_path_or_any_parents(relative, false)
                .is_ignore()
        {
            paths.push(relative.to_path_buf());
        }
    }
    paths.sort_by_key(|path| slash_lossy(path));
    Ok(Traversal { paths, diagnostics })
}

fn matcher(patterns: &[String]) -> Result<Gitignore, RepositoryError> {
    let mut builder = GitignoreBuilder::new("");
    for pattern in patterns {
        builder
            .add_line(None, pattern)
            .map_err(|error| RepositoryError::InvalidConfig(error.to_string()))?;
    }
    builder
        .build()
        .map_err(|error| RepositoryError::InvalidConfig(error.to_string()))
}

pub fn slash_path(path: &Path) -> Result<String, RepositoryError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_str().ok_or_else(|| {
                    RepositoryError::Traversal("path is not valid UTF-8".to_owned())
                })?);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(RepositoryError::Traversal(
                    "path escaped repository".to_owned(),
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

fn slash_lossy(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn internal(path: &str) -> bool {
    path == ".git"
        || path.starts_with(".git/")
        || path == ".pebble/pebble.toml"
        || path == ".pebble/local"
        || path.starts_with(".pebble/local/")
}
