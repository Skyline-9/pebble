//! Repository discovery, state, and projection contracts.

mod config;
mod config_fs;
mod git;
mod git_status;
mod identity;
mod layout;
mod registry;
mod registry_race;
mod snapshot;
mod traversal;

pub use config::RepositoryConfig;
pub use git::SystemGit;
pub use git_status::ChangedPath;
pub use layout::StateLayout;
pub use registry::{RegisteredRepository, RepositoryRegistry};
pub use snapshot::{RepositorySnapshot, SkipDiagnostic, SkipReason, SourceFile};

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::error::DomainError;

/// Failures at the repository and restricted Git boundaries.
#[derive(Debug, Error)]
pub enum RepositoryError {
    /// A filesystem operation failed.
    #[error("repository I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Repository configuration was invalid.
    #[error("invalid repository configuration: {0}")]
    InvalidConfig(String),
    /// Repository identity could not be derived from the remote.
    #[error("invalid Git remote: {0}")]
    InvalidRemote(String),
    /// A second checkout used a canonical ID without alternate-worktree consent.
    #[error("repository {repository} is already registered at {}", existing.display())]
    DuplicateCheckout {
        /// Conflicting canonical repository ID.
        repository: String,
        /// Existing registered checkout.
        existing: PathBuf,
    },
    /// The system Git executable was not found.
    #[error("Git executable was not found")]
    GitNotFound,
    /// A restricted Git command returned failure.
    #[error("Git {operation} failed: {message}")]
    GitFailed {
        /// Fixed operation that failed.
        operation: &'static str,
        /// Bounded standard error.
        message: String,
    },
    /// A restricted Git command exceeded its deadline.
    #[error("Git {0} timed out")]
    GitTimeout(&'static str),
    /// A restricted Git command exceeded its output budget.
    #[error("Git {0} exceeded its output limit")]
    GitOutputLimit(&'static str),
    /// Git returned malformed data.
    #[error("Git {operation} returned invalid output: {message}")]
    InvalidGitOutput {
        /// Fixed operation returning malformed output.
        operation: &'static str,
        /// Reason the output was malformed.
        message: String,
    },
    /// Repository traversal returned an invalid or inaccessible entry.
    #[error("repository traversal failed: {0}")]
    Traversal(String),
    /// The worktree changed after snapshot traversal began.
    #[error("repository worktree changed while reading snapshot")]
    WorktreeChanged,
    /// A derived domain value violated its invariant.
    #[error(transparent)]
    Domain(#[from] DomainError),
}

struct GitOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn bounded_output(
    mut command: Command,
    operation: &'static str,
    timeout: Duration,
    limit: usize,
) -> Result<GitOutput, RepositoryError> {
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| malformed_output(operation))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| malformed_output(operation))?;
    let (sender, receiver) = mpsc::sync_channel(0);
    spawn_reader(stdout, false, sender.clone());
    spawn_reader(stderr, true, sender.clone());
    drop(sender);
    let start = Instant::now();
    let (mut out, mut error) = (Vec::new(), Vec::new());
    let mut pipes_open = true;
    let mut status = None;
    loop {
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RepositoryError::GitTimeout(operation));
        }
        if pipes_open {
            let wait = timeout
                .saturating_sub(start.elapsed())
                .min(Duration::from_millis(5));
            match receiver.recv_timeout(wait) {
                Ok((is_error, bytes)) => {
                    if out.len() + error.len() + bytes.len() > limit {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(RepositoryError::GitOutputLimit(operation));
                    }
                    if is_error {
                        error.extend(bytes);
                    } else {
                        out.extend(bytes);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => pipes_open = false,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        } else {
            thread::sleep(
                timeout
                    .saturating_sub(start.elapsed())
                    .min(Duration::from_millis(5)),
            );
        }
        if status.is_none() {
            status = child.try_wait()?;
        }
        if let Some(status) = status
            && !pipes_open
        {
            return Ok(GitOutput {
                status,
                stdout: out,
                stderr: error,
            });
        }
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut pipe: R,
    is_error: bool,
    sender: mpsc::SyncSender<(bool, Vec<u8>)>,
) {
    thread::spawn(move || {
        let mut buffer = [0; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) if sender.send((is_error, buffer[..count].to_vec())).is_err() => break,
                Ok(_) => {}
            }
        }
    });
}

fn malformed_output(operation: &'static str) -> RepositoryError {
    RepositoryError::InvalidGitOutput {
        operation,
        message: "malformed record".to_owned(),
    }
}

#[cfg(unix)]
fn git_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn git_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::spawn_reader;

    struct CountingReader(Arc<AtomicUsize>);

    impl Read for CountingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.0.fetch_add(1, Ordering::SeqCst);
            buffer.fill(b'x');
            Ok(buffer.len())
        }
    }

    #[test]
    fn reader_cannot_buffer_an_unbounded_number_of_chunks() {
        let reads = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel(0);

        spawn_reader(CountingReader(Arc::clone(&reads)), false, sender);
        thread::sleep(Duration::from_millis(50));

        assert_eq!(reads.load(Ordering::SeqCst), 1);
        drop(receiver);
    }
}
