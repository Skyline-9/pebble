//! Bounded repository watching, event coalescing, and reconciliation.

mod coalesce;
mod retry;
#[cfg(test)]
mod retry_tests;
mod roots;
mod service;
#[cfg(test)]
mod tests;

use std::fmt::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use thiserror::Error;

use crate::domain::GenerationId;

pub use service::WatchService;

enum CallbackMessage {
    Path(PathBuf),
    Reconcile,
    Wake,
}

#[derive(Default)]
struct SharedSignals {
    overflow: AtomicBool,
    backend_error: Mutex<Option<String>>,
    terminal_error: Mutex<Option<WatchError>>,
}

impl SharedSignals {
    fn send_path(&self, sender: &mpsc::SyncSender<CallbackMessage>, path: PathBuf) {
        match sender.try_send(CallbackMessage::Path(path)) {
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                self.overflow.store(true, Ordering::Release);
            }
        }
    }

    fn request_reconciliation(&self, sender: &mpsc::SyncSender<CallbackMessage>) {
        match sender.try_send(CallbackMessage::Reconcile) {
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                self.overflow.store(true, Ordering::Release);
            }
        }
    }

    fn wake(sender: &mpsc::SyncSender<CallbackMessage>) {
        match sender.try_send(CallbackMessage::Wake) {
            Ok(()) | Err(mpsc::TrySendError::Full(_) | mpsc::TrySendError::Disconnected(_)) => {}
        }
    }

    fn report_backend_error(&self, message: impl fmt::Display) {
        let message = bounded_message(&message);
        let mut stored = lock(&self.backend_error);
        if stored.is_none() {
            *stored = Some(message);
        }
    }

    fn take_overflow(&self) -> bool {
        self.overflow.swap(false, Ordering::AcqRel)
    }

    fn take_backend_error(&self) -> Option<String> {
        lock(&self.backend_error).take()
    }

    fn store_terminal(&self, error: WatchError) {
        let mut terminal = lock(&self.terminal_error);
        if terminal.is_none() {
            *terminal = Some(error);
        }
    }

    fn take_terminal(&self) -> Option<WatchError> {
        lock(&self.terminal_error).take()
    }
}

/// One reason a watcher batch required conservative reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WatchDiagnostic {
    /// A bounded callback queue could not accept every path hint.
    QueueOverflow,
    /// The native watcher reported a backend error.
    BackendError(String),
}

/// One completed immutable repository generation build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionJob {
    generation: GenerationId,
    changed_paths: Vec<PathBuf>,
    full_scan: bool,
    diagnostics: Vec<WatchDiagnostic>,
}

impl RevisionJob {
    pub(crate) const fn new(
        generation: GenerationId,
        changed_paths: Vec<PathBuf>,
        full_scan: bool,
        diagnostics: Vec<WatchDiagnostic>,
    ) -> Self {
        Self {
            generation,
            changed_paths,
            full_scan,
            diagnostics,
        }
    }

    /// Return the newly activated immutable generation.
    #[must_use]
    pub const fn generation(&self) -> &GenerationId {
        &self.generation
    }

    /// Return sorted, normalized path hints coalesced for this build.
    ///
    /// A full-scan job can include no hints because the filesystem scan, not
    /// watcher delivery, is authoritative.
    #[must_use]
    pub fn changed_paths(&self) -> &[PathBuf] {
        &self.changed_paths
    }

    /// Return whether event loss or an explicit request forced full scanning.
    #[must_use]
    pub const fn full_scan(&self) -> bool {
        self.full_scan
    }

    /// Return bounded watcher diagnostics associated with this build.
    #[must_use]
    pub fn diagnostics(&self) -> &[WatchDiagnostic] {
        &self.diagnostics
    }
}

/// Failure to start, receive from, compile within, or stop a watcher service.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum WatchError {
    /// Repository or index path validation failed.
    #[error("watcher path validation failed: {0}")]
    InvalidPath(String),
    /// A configured include or exclude pattern could not be compiled.
    #[error("watcher path filter is invalid: {0}")]
    InvalidFilter(String),
    /// Native watcher setup failed.
    #[error("native watcher setup failed: {0}")]
    Notify(String),
    /// An immutable generation build failed.
    #[error("watcher generation build failed: {0}")]
    Compile(String),
    /// The bounded completed-job queue overflowed.
    #[error("watcher completed-job queue overflowed")]
    ResultQueueOverflow,
    /// The worker stopped without producing another job.
    #[error("watcher worker stopped")]
    WorkerStopped,
    /// The worker thread panicked during shutdown.
    #[error("watcher worker panicked")]
    WorkerPanicked,
}

impl WatchError {
    pub(crate) fn invalid_path(message: impl Into<String>) -> Self {
        Self::InvalidPath(message.into())
    }
}

/// Result type returned by watcher operations.
pub type WatchResult<T> = Result<T, WatchError>;

const QUIET_PERIOD: Duration = Duration::from_millis(250);
const CALLBACK_QUEUE_CAPACITY: usize = 256;
const RESULT_QUEUE_CAPACITY: usize = 8;
const MAX_COALESCED_PATHS: usize = 256;
const MAX_EVENT_PATHS: usize = 64;
const MAX_PATH_BYTES: usize = 4096;
const MAX_DIAGNOSTIC_BYTES: usize = 1024;

fn internal_path(path: &Path) -> bool {
    path == Path::new(".git")
        || path.starts_with(".git")
        || path == Path::new(".pebble/pebble.toml")
        || path == Path::new(".pebble/local")
        || path.starts_with(".pebble/local")
}

fn truncate_utf8(value: &mut String, maximum: usize) {
    if value.len() <= maximum {
        return;
    }
    let mut boundary = maximum;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

fn bounded_message(value: &impl fmt::Display) -> String {
    struct Writer(String);

    impl Write for Writer {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            let remaining = MAX_DIAGNOSTIC_BYTES.saturating_sub(self.0.len());
            let mut boundary = remaining.min(value.len());
            while !value.is_char_boundary(boundary) {
                boundary -= 1;
            }
            self.0.push_str(&value[..boundary]);
            Ok(())
        }
    }

    let mut writer = Writer(String::with_capacity(MAX_DIAGNOSTIC_BYTES));
    let _ = write!(writer, "{value}");
    writer.0
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
