//! Bounded local JSONL retrieval traces.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::domain::Citation;

use super::RetrievalError;

const MAX_TRACE_RECORD_BYTES: usize = 64 * 1024;
const MAX_TRACE_FILE_BYTES: u64 = 8 * 1024 * 1024;
static TRACE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// One selected candidate recorded without source text or query text.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TraceCandidate {
    entity_id: String,
    score: f32,
    citation: Citation,
}

impl TraceCandidate {
    pub(super) const fn new(entity_id: String, score: f32, citation: Citation) -> Self {
        Self {
            entity_id,
            score,
            citation,
        }
    }

    /// Return the selected entity ID.
    #[must_use]
    pub fn entity_id(&self) -> &str {
        &self.entity_id
    }

    /// Return the fused score.
    #[must_use]
    pub const fn score(&self) -> f32 {
        self.score
    }

    /// Return the resolved citation.
    #[must_use]
    pub const fn citation(&self) -> &Citation {
        &self.citation
    }
}

/// One candidate excluded from an evidence packet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OmittedCandidate {
    entity_id: String,
    reason: String,
}

impl OmittedCandidate {
    pub(super) fn new(entity_id: String, reason: impl Into<String>) -> Self {
        Self {
            entity_id,
            reason: reason.into(),
        }
    }

    /// Return the omitted entity ID.
    #[must_use]
    pub fn entity_id(&self) -> &str {
        &self.entity_id
    }

    /// Return the stable omission reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// Replayable local diagnostics for one model-free retrieval.
///
/// Query text and source excerpts are deliberately excluded.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryTrace {
    generation: String,
    active_scorers: Vec<String>,
    selected_candidates: Vec<TraceCandidate>,
    omitted_candidates: Vec<OmittedCandidate>,
    diagnostics: Vec<String>,
}

impl QueryTrace {
    pub(super) fn new(
        generation: String,
        active_scorers: Vec<String>,
        selected_candidates: Vec<TraceCandidate>,
        mut omitted_candidates: Vec<OmittedCandidate>,
        diagnostics: Vec<String>,
    ) -> Self {
        let mut seen = std::collections::BTreeSet::new();
        omitted_candidates.retain(|candidate| {
            seen.insert((candidate.entity_id.clone(), candidate.reason.clone()))
        });
        Self {
            generation,
            active_scorers,
            selected_candidates,
            omitted_candidates,
            diagnostics,
        }
    }

    /// Return the pinned generation ID.
    #[must_use]
    pub fn generation(&self) -> &str {
        &self.generation
    }

    /// Return scorer names that produced at least one filtered candidate.
    #[must_use]
    pub fn active_scorers(&self) -> &[String] {
        &self.active_scorers
    }

    /// Return selected candidates.
    #[must_use]
    pub fn selected_candidates(&self) -> &[TraceCandidate] {
        &self.selected_candidates
    }

    /// Return omitted candidates and decisions.
    #[must_use]
    pub fn omitted_candidates(&self) -> &[OmittedCandidate] {
        &self.omitted_candidates
    }

    /// Return nonfatal trace diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

pub(super) fn validate_path(path: &Path) -> Result<PathBuf, RetrievalError> {
    if !path.is_absolute()
        || path.file_name().is_none()
        || path.extension().and_then(|value| value.to_str()) != Some("jsonl")
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(RetrievalError::InvalidRequest(
            "trace path must be an absolute normalized .jsonl file".to_owned(),
        ));
    }
    reject_symlink_components(path)?;
    Ok(path.to_owned())
}

pub(super) fn append(path: &Path, trace: &QueryTrace) -> Result<(), RetrievalError> {
    let mut line = serde_json::to_vec(trace)?;
    line.push(b'\n');
    if line.len() > MAX_TRACE_RECORD_BYTES {
        return Err(RetrievalError::TraceBoundExceeded);
    }
    let lock = TRACE_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().map_err(|_| RetrievalError::TraceLockPoisoned)?;
    let parent = path.parent().ok_or_else(|| {
        RetrievalError::InvalidRequest("trace path has no parent directory".to_owned())
    })?;
    fs::create_dir_all(parent)?;
    reject_symlink_components(path)?;
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    set_no_follow(&mut options);
    let mut file = options.open(path)?;
    file.lock()?;
    if file
        .metadata()?
        .len()
        .saturating_add(u64::try_from(line.len()).unwrap_or(u64::MAX))
        > MAX_TRACE_FILE_BYTES
    {
        return Err(RetrievalError::TraceBoundExceeded);
    }
    file.write_all(&line)?;
    file.sync_data()?;
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), RetrievalError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(RetrievalError::InvalidRequest(
                    "trace path may not traverse symbolic links".to_owned(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
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
