use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use crate::index::IndexError;

use super::coalesce::Batch;
use super::{MAX_COALESCED_PATHS, QUIET_PERIOD};

pub(super) const MAX_CONSECUTIVE_COMPILE_FAILURES: usize = 4;
const MAX_BATCH_DIAGNOSTICS: usize = 2;

pub(super) enum Failure {
    Retry(Duration),
    Terminal(String),
}

pub(super) struct RetryState {
    pending: Option<Batch>,
    consecutive_failures: usize,
}

impl RetryState {
    pub(super) const fn new() -> Self {
        Self {
            pending: None,
            consecutive_failures: 0,
        }
    }

    pub(super) const fn note_event(&mut self) {
        self.consecutive_failures = 0;
    }

    pub(super) fn prepare(&mut self, batch: Batch) -> Batch {
        match self.pending.take() {
            Some(pending) => merge(pending, batch),
            None => batch,
        }
    }

    pub(super) fn failed(&mut self, batch: Batch, error: &IndexError) -> Failure {
        if !retryable(error) {
            return Failure::Terminal(error.to_string());
        }
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_CONSECUTIVE_COMPILE_FAILURES {
            return Failure::Terminal(error.to_string());
        }
        self.pending = Some(batch);
        let multiplier = u32::try_from(self.consecutive_failures).unwrap_or(u32::MAX);
        Failure::Retry(QUIET_PERIOD.saturating_mul(multiplier))
    }
}

const fn retryable(error: &IndexError) -> bool {
    matches!(error, IndexError::Repository(_))
}

fn merge(left: Batch, right: Batch) -> Batch {
    let mut paths = left
        .paths
        .into_iter()
        .chain(right.paths)
        .collect::<BTreeSet<PathBuf>>();
    if paths.len() > MAX_COALESCED_PATHS {
        paths.clear();
    }
    let mut diagnostics = left.diagnostics;
    for diagnostic in right.diagnostics {
        if diagnostics.len() < MAX_BATCH_DIAGNOSTICS && !diagnostics.contains(&diagnostic) {
            diagnostics.push(diagnostic);
        }
    }
    Batch {
        paths: paths.into_iter().collect(),
        full_scan: true,
        diagnostics,
    }
}
