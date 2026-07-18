//! Index health reporting and bounded service boundary helpers.

use std::fs::{self, Metadata, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::domain::{Citation, RepositoryId};
use crate::index::{GenerationReader, GraphCounts, IndexError};
use crate::repository::{RepositoryError, RepositoryRegistry};
use crate::retrieval::{QueryTrace, RetrievalError};
use crate::watcher::WatchError;

use super::{PebbleService, ServiceError};

const MAX_TRACE_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TRACE_RECORD_BYTES: usize = 64 * 1024;

#[cfg(test)]
mod tests;

/// Validated graph row totals exposed by health and indexing operations.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct HealthCounts {
    /// Indexed repositories.
    pub repositories: u64,
    /// Indexed revisions.
    pub revisions: u64,
    /// Indexed files.
    pub files: u64,
    /// Indexed symbols.
    pub symbols: u64,
    /// Indexed chunks.
    pub chunks: u64,
    /// Indexed structural edges.
    pub edges: u64,
    /// Indexed diagnostics.
    pub diagnostics: u64,
}

impl From<GraphCounts> for HealthCounts {
    fn from(counts: GraphCounts) -> Self {
        Self {
            repositories: counts.repositories(),
            revisions: counts.revisions(),
            files: counts.files(),
            symbols: counts.symbols(),
            chunks: counts.chunks(),
            edges: counts.edges(),
            diagnostics: counts.diagnostics(),
        }
    }
}

/// Read-only status of one repository's disposable projection.
#[derive(Clone, Debug, Serialize)]
pub struct HealthStatus {
    /// Repository being checked.
    pub repository_id: RepositoryId,
    /// Whether the current immutable generation is valid.
    pub healthy: bool,
    /// Current generation when valid.
    pub generation: Option<String>,
    /// Validated graph counts when healthy.
    pub counts: Option<HealthCounts>,
    /// Bounded explanation when unhealthy.
    pub issue: Option<String>,
}

impl PebbleService {
    /// Validate one repository's current `SQLite` and Tantivy generation.
    ///
    /// # Errors
    ///
    /// Returns an error only when the registry itself cannot be read safely.
    pub fn health(&self, repository: &RepositoryId) -> Result<HealthStatus, ServiceError> {
        if let Err(error) = RepositoryRegistry::new(self.state_root()).registrations() {
            return Err(map_repository_config(error));
        }
        if self.registration(repository).is_err() {
            return Ok(unhealthy(repository, "repository is not registered"));
        }
        match GenerationReader::open_current(&self.layout.generations(repository)) {
            Ok(reader) => match reader.graph().counts() {
                Ok(counts) => Ok(HealthStatus {
                    repository_id: repository.clone(),
                    healthy: true,
                    generation: Some(reader.id().to_string()),
                    counts: Some(counts.into()),
                    issue: None,
                }),
                Err(error) => Ok(unhealthy(repository, &error.to_string())),
            },
            Err(error) => Ok(unhealthy(repository, &error.to_string())),
        }
    }
}

fn unhealthy(repository: &RepositoryId, issue: &str) -> HealthStatus {
    HealthStatus {
        repository_id: repository.clone(),
        healthy: false,
        generation: None,
        counts: None,
        issue: Some(bounded(issue, 2_048)),
    }
}

pub(super) fn canonical_repository(repository: &Path) -> Result<PathBuf, ServiceError> {
    let metadata = fs::symlink_metadata(repository).map_err(ServiceError::operational)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ServiceError::configuration(
            "repository must be a real directory",
        ));
    }
    repository.canonicalize().map_err(ServiceError::operational)
}

pub(super) fn map_repository(error: RepositoryError) -> ServiceError {
    ServiceError::operational(error)
}

pub(super) fn map_repository_config(error: RepositoryError) -> ServiceError {
    match error {
        RepositoryError::InvalidConfig(_)
        | RepositoryError::InvalidRemote(_)
        | RepositoryError::DuplicateCheckout { .. }
        | RepositoryError::Domain(_) => ServiceError::configuration(error),
        _ => ServiceError::operational(error),
    }
}

pub(super) fn map_index_operational(error: IndexError) -> ServiceError {
    ServiceError::operational(error)
}

pub(super) fn map_index_unavailable(error: IndexError) -> ServiceError {
    ServiceError::unavailable(error)
}

pub(super) fn map_retrieval(error: RetrievalError) -> ServiceError {
    match error {
        RetrievalError::InvalidRequest(_) | RetrievalError::Domain(_) => ServiceError::usage(error),
        RetrievalError::Index(_) => ServiceError::unavailable(error),
        _ => ServiceError::operational(error),
    }
}

pub(super) fn map_watch(error: WatchError) -> ServiceError {
    ServiceError::operational(error)
}

pub(super) fn ensure_indexed_citation(
    reader: &GenerationReader,
    citation: &Citation,
) -> Result<(), ServiceError> {
    let connection = Connection::open_with_flags(
        reader.graph_path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|error| ServiceError::unavailable(error.to_string()))?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|error| ServiceError::unavailable(error.to_string()))?;
    let exists = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM files
                WHERE generation_id = ?1 AND repository_id = ?2
                  AND revision = ?3 AND path = ?4
            )",
            (
                reader.id().as_str(),
                citation.repository().as_str(),
                citation.revision().to_string(),
                citation.path(),
            ),
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| ServiceError::unavailable(error.to_string()))?;
    if exists {
        Ok(())
    } else {
        Err(ServiceError::stale(
            "citation is not present in the active generation",
        ))
    }
}

pub(super) fn exact_lines(source: &str, start: u32, end: u32) -> Option<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let start = usize::try_from(start).ok()?.checked_sub(1)?;
    let end = usize::try_from(end).ok()?;
    (start < end && end <= lines.len()).then(|| lines[start..end].join("\n"))
}

pub(super) fn read_traces(path: &Path, limit: usize) -> Result<Vec<QueryTrace>, ServiceError> {
    let mut options = OpenOptions::new();
    options.read(true);
    no_follow(&mut options);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ServiceError::operational(error)),
    };
    let opened = file.metadata().map_err(ServiceError::operational)?;
    let path_metadata = fs::symlink_metadata(path).map_err(ServiceError::operational)?;
    if !opened.is_file()
        || opened.len() > MAX_TRACE_FILE_BYTES
        || !same_file(&opened, &path_metadata)
    {
        return Err(unsafe_trace());
    }
    super::trace_race::run(path);
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    file.by_ref()
        .take(MAX_TRACE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(ServiceError::operational)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_TRACE_FILE_BYTES {
        return Err(unsafe_trace());
    }
    let after = file.metadata().map_err(ServiceError::operational)?;
    let path_after = fs::symlink_metadata(path).map_err(ServiceError::operational)?;
    if !same_file(&opened, &after) || !same_file(&opened, &path_after) {
        return Err(unsafe_trace());
    }
    let mut traces = Vec::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if line.len() > MAX_TRACE_RECORD_BYTES {
            return Err(ServiceError::operational("trace record exceeds 64 KiB"));
        }
        traces.push(serde_json::from_slice(line).map_err(ServiceError::operational)?);
    }
    let keep_from = traces.len().saturating_sub(limit);
    Ok(traces.split_off(keep_from))
}

fn unsafe_trace() -> ServiceError {
    ServiceError::operational("trace file is unsafe, changed, or exceeds 8 MiB")
}

fn bounded(value: &str, maximum: usize) -> String {
    let mut boundary = value.len().min(maximum);
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

#[cfg(unix)]
fn no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(no_follow_flag());
}

#[cfg(not(unix))]
const fn no_follow(_options: &mut OpenOptions) {}

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
