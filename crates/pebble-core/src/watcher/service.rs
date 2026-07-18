use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::index::{IndexError, RepositoryCompiler};
use crate::repository::RepositoryConfig;

use super::coalesce::{Batch, Coalescer, Input, PathFilter, handle_event};
use super::retry::{Failure, RetryState};
use super::{
    CALLBACK_QUEUE_CAPACITY, CallbackMessage, QUIET_PERIOD, RESULT_QUEUE_CAPACITY, RevisionJob,
    SharedSignals, WatchError, WatchResult,
};

/// One native recursive watcher and one bounded reconciliation worker.
pub struct WatchService {
    watcher: Option<RecommendedWatcher>,
    sender: mpsc::SyncSender<CallbackMessage>,
    receiver: mpsc::Receiver<WatchResult<RevisionJob>>,
    signals: Arc<SharedSignals>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

struct Worker {
    repository: PathBuf,
    generations: PathBuf,
    config: RepositoryConfig,
    input: mpsc::Receiver<CallbackMessage>,
    results: mpsc::SyncSender<WatchResult<RevisionJob>>,
    signals: Arc<SharedSignals>,
    cancelled: Arc<AtomicBool>,
}

impl WatchService {
    /// Start watching one real repository root and compile complete generations.
    ///
    /// Native callbacks only normalize and enqueue bounded path hints. The
    /// worker waits for a 250 ms quiet period, then scans and compiles the
    /// repository through [`RepositoryCompiler`].
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe roots, invalid filters, native watcher
    /// initialization, or thread creation failure.
    pub fn start(
        repository: &Path,
        generations: &Path,
        config: RepositoryConfig,
    ) -> WatchResult<Self> {
        let (repository, generations) = super::roots::validate(repository, generations)?;
        let filter = Arc::new(PathFilter::new(&config)?);
        let (sender, input) = mpsc::sync_channel(CALLBACK_QUEUE_CAPACITY);
        let signals = Arc::new(SharedSignals::default());
        let callback_sender = sender.clone();
        let callback_signals = Arc::clone(&signals);
        let callback_root = repository.clone();
        let mut watcher = notify::recommended_watcher(move |result| {
            handle_event(
                result,
                &callback_root,
                &filter,
                &callback_sender,
                &callback_signals,
            );
        })
        .map_err(|error| WatchError::Notify(error.to_string()))?;
        watcher
            .watch(&repository, RecursiveMode::Recursive)
            .map_err(|error| WatchError::Notify(error.to_string()))?;

        let (results, receiver) = mpsc::sync_channel(RESULT_QUEUE_CAPACITY);
        let cancelled = Arc::new(AtomicBool::new(false));
        let state = Worker {
            repository,
            generations,
            config,
            input,
            results,
            signals: Arc::clone(&signals),
            cancelled: Arc::clone(&cancelled),
        };
        let worker = thread::Builder::new()
            .name("pebble-watch".to_owned())
            .spawn(move || state.run())
            .map_err(|error| WatchError::Notify(error.to_string()))?;
        Ok(Self {
            watcher: Some(watcher),
            sender,
            receiver,
            signals,
            cancelled,
            worker: Some(worker),
        })
    }

    /// Request conservative full-scan reconciliation after the quiet period.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker has already stopped.
    pub fn request_reconciliation(&self) -> WatchResult<()> {
        match self.sender.try_send(CallbackMessage::Reconcile) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                self.signals.overflow.store(true, Ordering::Release);
                SharedSignals::wake(&self.sender);
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(_)) => Err(WatchError::WorkerStopped),
        }
    }

    /// Wait up to `timeout` for one completed revision job.
    ///
    /// # Errors
    ///
    /// Returns a retained worker or compilation error, including errors that
    /// could not fit in the bounded result queue.
    pub fn recv_timeout(&self, timeout: Duration) -> WatchResult<Option<RevisionJob>> {
        if let Some(error) = self.signals.take_terminal() {
            return Err(error);
        }
        match self.receiver.recv_timeout(timeout) {
            Ok(result) => result.map(Some),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.signals.take_terminal().map_or(Ok(None), Err)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self
                .signals
                .take_terminal()
                .unwrap_or(WatchError::WorkerStopped)),
        }
    }

    /// Stop native delivery, cancel pending work, and join the sole worker.
    ///
    /// # Errors
    ///
    /// Returns a retained terminal worker error or panic.
    pub fn shutdown(&mut self) -> WatchResult<()> {
        self.stop()
    }

    fn stop(&mut self) -> WatchResult<()> {
        drop(self.watcher.take());
        self.cancelled.store(true, Ordering::Release);
        SharedSignals::wake(&self.sender);
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            return Err(WatchError::WorkerPanicked);
        }
        if let Some(error) = self.signals.take_terminal() {
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for WatchService {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl Worker {
    fn run(self) {
        let compiler = RepositoryCompiler::new(&self.generations);
        let mut coalescer = Coalescer::new(QUIET_PERIOD);
        let mut retry = RetryState::new();
        loop {
            if drain_signals(&mut coalescer, &self.signals) {
                retry.note_event();
            }
            if self.cancelled.load(Ordering::Acquire) {
                return;
            }
            let wait = coalescer
                .deadline()
                .map_or(Duration::from_millis(50), |deadline| {
                    deadline.saturating_duration_since(Instant::now())
                });
            match self.input.recv_timeout(wait) {
                Ok(CallbackMessage::Path(path)) => {
                    retry.note_event();
                    coalescer.record(Input::Paths(vec![path]), Instant::now());
                }
                Ok(CallbackMessage::Reconcile) => {
                    retry.note_event();
                    coalescer.record(Input::Reconcile, Instant::now());
                }
                Ok(CallbackMessage::Wake) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if drain_signals(&mut coalescer, &self.signals) {
                        retry.note_event();
                    }
                    if coalescer
                        .deadline()
                        .is_some_and(|deadline| deadline <= Instant::now())
                        && let Some(batch) = coalescer.take()
                    {
                        let batch = retry.prepare(batch);
                        match compile_batch(
                            &compiler,
                            &self.repository,
                            &self.config,
                            batch,
                            &self.results,
                            &self.signals,
                        ) {
                            CompileOutcome::Continue => {}
                            CompileOutcome::Retry(batch, error) => {
                                match retry.failed(batch, &error) {
                                    Failure::Retry(delay) => {
                                        coalescer.record(Input::Retry(delay), Instant::now());
                                    }
                                    Failure::Terminal(message) => {
                                        self.signals.store_terminal(WatchError::Compile(message));
                                        return;
                                    }
                                }
                            }
                            CompileOutcome::Stop => return,
                        }
                    }
                }
            }
        }
    }
}

fn drain_signals(coalescer: &mut Coalescer, signals: &SharedSignals) -> bool {
    let now = Instant::now();
    let overflow = signals.take_overflow();
    if overflow {
        coalescer.record(Input::Overflow, now);
    }
    let backend_error = signals.take_backend_error();
    let recorded = overflow || backend_error.is_some();
    if let Some(error) = backend_error {
        coalescer.record(Input::BackendError(error), now);
    }
    recorded
}

enum CompileOutcome {
    Continue,
    Retry(Batch, IndexError),
    Stop,
}

fn compile_batch(
    compiler: &RepositoryCompiler,
    repository: &Path,
    config: &RepositoryConfig,
    batch: Batch,
    results: &mpsc::SyncSender<WatchResult<RevisionJob>>,
    signals: &SharedSignals,
) -> CompileOutcome {
    let reader = match compiler.compile_fresh(repository, config) {
        Ok(reader) => reader,
        Err(error) => return CompileOutcome::Retry(batch, error),
    };
    let job = RevisionJob::new(
        reader.id().clone(),
        batch.paths,
        batch.full_scan,
        batch.diagnostics,
    );
    match results.try_send(Ok(job)) {
        Ok(()) => CompileOutcome::Continue,
        Err(mpsc::TrySendError::Disconnected(_)) => CompileOutcome::Stop,
        Err(mpsc::TrySendError::Full(result)) => {
            signals.store_terminal(result.err().unwrap_or(WatchError::ResultQueueOverflow));
            CompileOutcome::Stop
        }
    }
}
