use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use notify::Event;
use notify::event::{CreateKind, EventKind, Flag};

use super::coalesce::{
    Batch, Coalescer, Input, PathDecision, PathFilter, directory_requires_scan, handle_event,
    normalize_path,
};
use super::{CallbackMessage, MAX_COALESCED_PATHS, SharedSignals, WatchDiagnostic};
use crate::repository::RepositoryConfig;

#[test]
fn coalesces_normalized_create_modify_remove_rename_and_atomic_hints() {
    let started = Instant::now();
    let mut coalescer = Coalescer::new(Duration::from_millis(250));
    coalescer.record(
        Input::Paths(vec![
            PathBuf::from("created.rs"),
            PathBuf::from("created.rs"),
            PathBuf::from("removed.rs"),
        ]),
        started,
    );
    coalescer.record(
        Input::Paths(vec![
            PathBuf::from("before.rs"),
            PathBuf::from("after.rs"),
            PathBuf::from(".atomic.tmp"),
            PathBuf::from("atomic.rs"),
        ]),
        started + Duration::from_millis(100),
    );

    assert_eq!(
        coalescer.deadline(),
        Some(started + Duration::from_millis(350))
    );
    assert_eq!(
        coalescer.take(),
        Some(Batch {
            paths: vec![
                PathBuf::from(".atomic.tmp"),
                PathBuf::from("after.rs"),
                PathBuf::from("atomic.rs"),
                PathBuf::from("before.rs"),
                PathBuf::from("created.rs"),
                PathBuf::from("removed.rs"),
            ],
            full_scan: false,
            diagnostics: Vec::new(),
        })
    );
}

#[test]
fn overflow_backend_error_and_reconciliation_are_sticky_and_bounded() {
    let started = Instant::now();
    let mut coalescer = Coalescer::new(Duration::from_millis(250));
    coalescer.record(Input::Overflow, started);
    coalescer.record(Input::Reconcile, started);
    coalescer.record(
        Input::BackendError("x".repeat(2_000)),
        started + Duration::from_millis(10),
    );
    coalescer.record(
        Input::BackendError("second".to_owned()),
        started + Duration::from_millis(20),
    );

    let batch = coalescer.take();
    assert!(batch.is_some(), "batch must exist");
    let Some(batch) = batch else {
        return;
    };
    assert!(batch.full_scan);
    assert_eq!(batch.diagnostics[0], WatchDiagnostic::QueueOverflow);
    assert!(matches!(
        &batch.diagnostics[1],
        WatchDiagnostic::BackendError(message) if message.len() == 1_024
    ));
    assert_eq!(batch.diagnostics.len(), 2);
}

#[test]
fn unique_path_storm_discards_hints_and_forces_a_bounded_full_scan() {
    let started = Instant::now();
    let mut coalescer = Coalescer::new(Duration::from_millis(250));
    for index in 0..=MAX_COALESCED_PATHS {
        coalescer.record(
            Input::Paths(vec![PathBuf::from(format!("src/{index}.rs"))]),
            started,
        );
    }

    let batch = coalescer.take();
    assert!(batch.is_some(), "batch must exist");
    let Some(batch) = batch else {
        return;
    };
    assert!(batch.full_scan);
    assert!(batch.paths.is_empty());
}

#[test]
fn callback_ingress_marks_overflow_and_preserves_one_bounded_error() {
    let signals = Arc::new(SharedSignals::default());
    let (sender, _receiver) = mpsc::sync_channel(1);
    assert!(
        sender
            .try_send(CallbackMessage::Path(PathBuf::from("occupied")))
            .is_ok(),
        "fixture queue has capacity"
    );

    signals.send_path(&sender, PathBuf::from("overflowed"));
    signals.report_backend_error("é".repeat(1_000));
    signals.report_backend_error("second".to_owned());

    assert!(signals.take_overflow());
    let message = signals.take_backend_error();
    assert!(message.is_some(), "error retained");
    let Some(message) = message else {
        return;
    };
    assert!(message.len() <= 1_024);
    assert!(message.is_char_boundary(message.len()));
    assert_eq!(signals.take_backend_error(), None);
}

#[test]
fn native_rescan_notice_requests_full_reconciliation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = ConfigFixture::new()?;
    let config = RepositoryConfig::load(&fixture.path)?;
    let filter = PathFilter::new(&config)?;
    let signals = SharedSignals::default();
    let (sender, _receiver) = mpsc::sync_channel(1);
    let event = Event::new(EventKind::Any).set_flag(Flag::Rescan);

    handle_event(
        Ok(event),
        Path::new("/repository"),
        &filter,
        &sender,
        &signals,
    );

    assert!(signals.take_overflow());
    Ok(())
}

#[test]
fn directory_creation_requests_full_reconciliation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = ConfigFixture::new()?;
    let config = RepositoryConfig::load(&fixture.path)?;
    let filter = PathFilter::new(&config)?;
    let signals = SharedSignals::default();
    let (sender, receiver) = mpsc::sync_channel(1);
    let event = Event::new(EventKind::Create(CreateKind::Folder))
        .add_path(PathBuf::from("/repository/src/new"));

    handle_event(
        Ok(event),
        Path::new("/repository"),
        &filter,
        &sender,
        &signals,
    );

    assert!(matches!(
        receiver.try_recv(),
        Ok(CallbackMessage::Reconcile)
    ));
    Ok(())
}

#[test]
fn ambiguous_create_reconciles_while_file_create_preserves_path_hint()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ConfigFixture::new()?;
    let config = RepositoryConfig::load(&fixture.path)?;
    let filter = PathFilter::new(&config)?;

    let signals = SharedSignals::default();
    let (sender, receiver) = mpsc::sync_channel(1);
    let ambiguous = Event::new(EventKind::Create(CreateKind::Any))
        .add_path(PathBuf::from("/repository/src/new"));
    handle_event(
        Ok(ambiguous),
        Path::new("/repository"),
        &filter,
        &sender,
        &signals,
    );
    assert!(matches!(
        receiver.try_recv(),
        Ok(CallbackMessage::Reconcile)
    ));

    let signals = SharedSignals::default();
    let (sender, receiver) = mpsc::sync_channel(1);
    let file = Event::new(EventKind::Create(CreateKind::File))
        .add_path(PathBuf::from("/repository/src/new.rs"));
    handle_event(
        Ok(file),
        Path::new("/repository"),
        &filter,
        &sender,
        &signals,
    );
    assert!(matches!(
        receiver.try_recv(),
        Ok(CallbackMessage::Path(path)) if path == Path::new("src/new.rs")
    ));
    Ok(())
}

#[test]
fn normalization_rejects_escapes_internal_paths_and_filter_misses()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new("/repository");
    let fixture = ConfigFixture::new()?;
    let config = RepositoryConfig::load(&fixture.path)?;
    let filter = PathFilter::new(&config)?;

    assert_eq!(
        normalize_path(root, Path::new("/repository/src/lib.rs"), &filter),
        PathDecision::Hint(PathBuf::from("src/lib.rs"))
    );
    assert_eq!(
        normalize_path(root, Path::new("/repository/src/../escape"), &filter),
        PathDecision::Reconcile
    );
    assert_eq!(
        normalize_path(root, Path::new("/repository/.git/index"), &filter),
        PathDecision::Ignore
    );
    assert_eq!(
        normalize_path(root, Path::new("/repository/.pebble/local/state"), &filter),
        PathDecision::Ignore
    );
    assert_eq!(
        normalize_path(
            root,
            Path::new("/repository/src/generated/output.rs"),
            &filter
        ),
        PathDecision::Ignore
    );
    assert!(!directory_requires_scan(
        root,
        Path::new("/repository/src/generated"),
        &filter
    ));
    assert_eq!(
        normalize_path(root, Path::new("/repository/docs/readme.md"), &filter),
        PathDecision::Ignore
    );
    Ok(())
}

struct ConfigFixture {
    path: PathBuf,
}

impl ConfigFixture {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "pebble-watcher-filter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join(".pebble"))?;
        fs::write(
            path.join(".pebble/pebble.toml"),
            "schema = 1\n\
             repository_id = \"watcher.test\"\n\
             include = [\"src/**\"]\n\
             exclude = [\"src/generated/**\"]\n\n\
             [language_overrides]\n",
        )?;
        Ok(Self { path })
    }
}

impl Drop for ConfigFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
