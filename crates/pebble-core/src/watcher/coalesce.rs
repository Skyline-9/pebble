use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::Event;
use notify::event::{CreateKind, EventKind};

use crate::repository::RepositoryConfig;

use super::{
    CallbackMessage, MAX_COALESCED_PATHS, MAX_EVENT_PATHS, MAX_PATH_BYTES, SharedSignals,
    WatchDiagnostic, WatchError, WatchResult, internal_path, truncate_utf8,
};

pub(super) struct PathFilter {
    include: Gitignore,
    exclude: Gitignore,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum PathDecision {
    Hint(PathBuf),
    Ignore,
    Reconcile,
}

impl PathFilter {
    pub(super) fn new(config: &RepositoryConfig) -> WatchResult<Self> {
        Ok(Self {
            include: matcher(config.include())?,
            exclude: matcher(config.exclude())?,
        })
    }

    fn accepts(&self, path: &Path) -> bool {
        let included = self
            .include
            .matched_path_or_any_parents(path, false)
            .is_ignore()
            || self
                .include
                .matched_path_or_any_parents(path, true)
                .is_ignore();
        included && !self.excludes(path)
    }

    fn excludes(&self, path: &Path) -> bool {
        self.exclude
            .matched_path_or_any_parents(path, false)
            .is_ignore()
            || self
                .exclude
                .matched_path_or_any_parents(path, true)
                .is_ignore()
    }
}

pub(super) fn handle_event(
    result: notify::Result<Event>,
    root: &Path,
    filter: &PathFilter,
    sender: &mpsc::SyncSender<CallbackMessage>,
    signals: &SharedSignals,
) {
    match result {
        Ok(event) if event.need_rescan() => {
            signals.overflow.store(true, Ordering::Release);
            SharedSignals::wake(sender);
        }
        Ok(Event {
            kind: EventKind::Access(_),
            ..
        }) => {}
        Ok(event) if event.paths.len() <= MAX_EVENT_PATHS => {
            if matches!(
                event.kind,
                EventKind::Create(CreateKind::Any | CreateKind::Folder)
            ) {
                if event
                    .paths
                    .iter()
                    .any(|path| directory_requires_scan(root, path, filter))
                {
                    signals.request_reconciliation(sender);
                }
                return;
            }
            for path in event.paths {
                match normalize_path(root, &path, filter) {
                    PathDecision::Hint(path) => signals.send_path(sender, path),
                    PathDecision::Ignore => {}
                    PathDecision::Reconcile => {
                        signals.overflow.store(true, Ordering::Release);
                        SharedSignals::wake(sender);
                    }
                }
            }
        }
        Ok(_) => {
            signals.overflow.store(true, Ordering::Release);
            SharedSignals::wake(sender);
        }
        Err(error) => {
            signals.report_backend_error(error);
            SharedSignals::wake(sender);
        }
    }
}

pub(super) fn directory_requires_scan(root: &Path, path: &Path, filter: &PathFilter) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    !internal_path(relative) && !filter.excludes(&relative.join("pebble-watch-child"))
}

pub(super) fn normalize_path(root: &Path, path: &Path, filter: &PathFilter) -> PathDecision {
    let Ok(relative) = path.strip_prefix(root) else {
        return PathDecision::Reconcile;
    };
    if relative.as_os_str().is_empty() {
        return PathDecision::Ignore;
    }
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return PathDecision::Reconcile;
    }
    let Some(encoded) = relative.to_str() else {
        return PathDecision::Reconcile;
    };
    if encoded.len() > MAX_PATH_BYTES {
        return PathDecision::Reconcile;
    }
    if internal_path(relative) || !filter.accepts(relative) {
        return PathDecision::Ignore;
    }
    if filter.excludes(&relative.join("pebble-watch-child")) {
        return PathDecision::Ignore;
    }
    PathDecision::Hint(relative.to_path_buf())
}

fn matcher(patterns: &[String]) -> WatchResult<Gitignore> {
    let mut builder = GitignoreBuilder::new("");
    for pattern in patterns {
        builder
            .add_line(None, pattern)
            .map_err(|error| WatchError::InvalidFilter(error.to_string()))?;
    }
    builder
        .build()
        .map_err(|error| WatchError::InvalidFilter(error.to_string()))
}

pub(super) enum Input {
    Paths(Vec<PathBuf>),
    Reconcile,
    Retry(Duration),
    Overflow,
    BackendError(String),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Batch {
    pub(super) paths: Vec<PathBuf>,
    pub(super) full_scan: bool,
    pub(super) diagnostics: Vec<WatchDiagnostic>,
}

pub(super) struct Coalescer {
    quiet_period: Duration,
    paths: BTreeSet<PathBuf>,
    full_scan: bool,
    overflow: bool,
    backend_error: Option<String>,
    deadline: Option<Instant>,
}

impl Coalescer {
    pub(super) const fn new(quiet_period: Duration) -> Self {
        Self {
            quiet_period,
            paths: BTreeSet::new(),
            full_scan: false,
            overflow: false,
            backend_error: None,
            deadline: None,
        }
    }

    pub(super) fn record(&mut self, input: Input, now: Instant) {
        let delay = match &input {
            Input::Retry(delay) => *delay,
            _ => self.quiet_period,
        };
        match input {
            Input::Paths(paths) if !self.full_scan => {
                for path in paths {
                    self.paths.insert(path);
                    if self.paths.len() > MAX_COALESCED_PATHS {
                        self.paths.clear();
                        self.full_scan = true;
                        break;
                    }
                }
            }
            Input::Paths(_) => {}
            Input::Reconcile | Input::Retry(_) => self.full_scan = true,
            Input::Overflow => {
                self.full_scan = true;
                self.overflow = true;
            }
            Input::BackendError(mut message) => {
                self.full_scan = true;
                if self.backend_error.is_none() {
                    truncate_utf8(&mut message, super::MAX_DIAGNOSTIC_BYTES);
                    self.backend_error = Some(message);
                }
            }
        }
        self.deadline = Some(now + delay);
    }

    pub(super) const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(super) fn take(&mut self) -> Option<Batch> {
        self.deadline.take()?;
        let mut diagnostics = Vec::with_capacity(2);
        if std::mem::take(&mut self.overflow) {
            diagnostics.push(WatchDiagnostic::QueueOverflow);
        }
        if let Some(message) = self.backend_error.take() {
            diagnostics.push(WatchDiagnostic::BackendError(message));
        }
        Some(Batch {
            paths: std::mem::take(&mut self.paths).into_iter().collect(),
            full_scan: std::mem::take(&mut self.full_scan),
            diagnostics,
        })
    }
}
