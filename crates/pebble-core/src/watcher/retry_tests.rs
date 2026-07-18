use std::path::PathBuf;
use std::time::Duration;

use crate::index::IndexError;
use crate::repository::RepositoryError;

use super::coalesce::Batch;
use super::retry::{Failure, MAX_CONSECUTIVE_COMPILE_FAILURES, RetryState};

#[test]
fn transient_snapshot_failures_retry_then_preserve_the_event_on_success() {
    let mut retry = RetryState::new();
    let mut next = batch_with_path("src/changed.rs");
    let mut attempts = 0;

    let completed = loop {
        attempts += 1;
        let batch = retry.prepare(next);
        if attempts == 3 {
            break batch;
        }
        let error = if attempts == 1 {
            RepositoryError::WorktreeChanged
        } else {
            RepositoryError::Traversal("file changed while reading: src/changed.rs".to_owned())
        };
        match retry.failed(batch, &IndexError::Repository(error)) {
            Failure::Retry(delay) => {
                assert_eq!(delay, Duration::from_millis(250 * attempts));
                next = full_scan();
            }
            Failure::Terminal(message) => {
                assert!(message.is_empty(), "unexpected terminal failure: {message}");
                return;
            }
        }
    };

    assert_eq!(attempts, 3);
    assert!(completed.full_scan);
    assert_eq!(completed.paths, vec![PathBuf::from("src/changed.rs")]);
}

#[test]
fn transient_snapshot_failures_have_an_explicit_terminal_limit() {
    let mut retry = RetryState::new();
    let error = IndexError::Repository(RepositoryError::WorktreeChanged);
    let mut batch = batch_with_path("src/changed.rs");
    for attempt in 1..=MAX_CONSECUTIVE_COMPILE_FAILURES {
        let failure = retry.failed(batch, &error);
        if attempt == MAX_CONSECUTIVE_COMPILE_FAILURES {
            assert!(matches!(failure, Failure::Terminal(_)));
            return;
        }
        assert!(matches!(failure, Failure::Retry(_)));
        batch = retry.prepare(full_scan());
    }
}

#[test]
fn a_new_event_resets_the_consecutive_retry_backoff() {
    let mut retry = RetryState::new();
    let error = IndexError::Repository(RepositoryError::WorktreeChanged);
    let first = retry.failed(batch_with_path("src/first.rs"), &error);
    assert!(matches!(
        first,
        Failure::Retry(delay) if delay == Duration::from_millis(250)
    ));

    retry.note_event();
    let merged = retry.prepare(batch_with_path("src/second.rs"));
    let reset = retry.failed(merged, &error);

    assert!(matches!(
        reset,
        Failure::Retry(delay) if delay == Duration::from_millis(250)
    ));
}

fn batch_with_path(path: &str) -> Batch {
    Batch {
        paths: vec![PathBuf::from(path)],
        full_scan: false,
        diagnostics: Vec::new(),
    }
}

fn full_scan() -> Batch {
    Batch {
        paths: Vec::new(),
        full_scan: true,
        diagnostics: Vec::new(),
    }
}
