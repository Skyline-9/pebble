//! Immutable generation publication and recovery integration tests.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use pebble_core::domain::{GenerationId, RepositoryId};
use pebble_core::index::{GenerationBuilder, GenerationReader, IndexError};

#[test]
fn builds_seals_and_atomically_activates_a_generation() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("publish")?;
    let builder = GenerationBuilder::create(directory.path(), generation("first")?)?;
    assert!(directory.path().join("first.building").is_dir());
    assert!(!directory.path().join("CURRENT").exists());

    builder
        .graph()
        .insert_repository(&repository()?, "Pebble")?;
    let sealed = builder.seal()?;
    assert!(directory.path().join("first").is_dir());
    assert!(!directory.path().join("first.building").exists());
    assert!(!directory.path().join("CURRENT").exists());

    let reader = sealed.activate()?;
    assert_eq!(reader.id().as_str(), "first");
    assert_eq!(
        fs::read_to_string(directory.path().join("CURRENT"))?,
        "first\n"
    );
    assert!(!directory.path().join("CURRENT.tmp").exists());
    assert_eq!(reader.graph().counts()?.repositories(), 1);
    Ok(())
}

#[test]
fn missing_or_corrupt_current_requires_a_rebuild() -> Result<(), Box<dyn std::error::Error>> {
    let missing = TestDirectory::new("missing-current")?;
    assert!(matches!(
        GenerationReader::open_current(missing.path()),
        Err(IndexError::RebuildRequired(_))
    ));

    fs::write(missing.path().join("CURRENT"), "../escape\n")?;
    assert!(matches!(
        GenerationReader::open_current(missing.path()),
        Err(IndexError::RebuildRequired(_))
    ));

    fs::write(missing.path().join("CURRENT"), "absent\n")?;
    assert!(matches!(
        GenerationReader::open_current(missing.path()),
        Err(IndexError::RebuildRequired(_))
    ));
    Ok(())
}

#[test]
fn incomplete_builds_are_ignored_without_deleting_unowned_directories()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("incomplete")?;
    publish(directory.path(), "stable", "stable-repository")?;
    let portable = directory.path().join("portable-source.jsonl");
    fs::write(&portable, "source of truth\n")?;
    let incomplete = directory.path().join("sources.building");
    fs::create_dir(&incomplete)?;
    fs::write(incomplete.join("portable-source.jsonl"), "partial")?;
    fs::write(directory.path().join("CURRENT.tmp"), "interrupted\n")?;

    let reader = GenerationReader::open_current(directory.path())?;

    assert_eq!(reader.id().as_str(), "stable");
    assert!(incomplete.is_dir());
    assert_eq!(
        fs::read_to_string(incomplete.join("portable-source.jsonl"))?,
        "partial"
    );
    assert!(directory.path().join("CURRENT.tmp").is_file());
    assert_eq!(fs::read_to_string(portable)?, "source of truth\n");
    Ok(())
}

#[test]
fn opening_current_does_not_race_an_active_build() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("active-build")?;
    publish(directory.path(), "stable", "stable-repository")?;
    let builder = GenerationBuilder::create(directory.path(), generation("active")?)?;

    let reader = GenerationReader::open_current(directory.path())?;
    builder
        .graph()
        .insert_repository(&repository()?, "active repository")?;

    assert_eq!(reader.id().as_str(), "stable");
    assert!(directory.path().join("active.building").is_dir());
    assert_eq!(
        builder.seal()?.activate()?.graph().counts()?.repositories(),
        1
    );
    Ok(())
}

#[test]
fn current_reads_are_bounded_and_reject_symlinks() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("bounded-current")?;
    let current = directory.path().join("CURRENT");
    let oversized = fs::File::create(&current)?;
    oversized.set_len(1 << 30)?;
    assert!(matches!(
        GenerationReader::open_current(directory.path()),
        Err(IndexError::RebuildRequired(_))
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        fs::remove_file(&current)?;
        let target = directory.path().join("current-target");
        fs::write(&target, "absent\n")?;
        symlink(&target, &current)?;
        assert!(matches!(
            GenerationReader::open_current(directory.path()),
            Err(IndexError::RebuildRequired(_))
        ));
    }
    Ok(())
}

#[test]
fn activation_does_not_follow_legacy_current_temp_symlinks()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("temp-symlink")?;
    let builder = GenerationBuilder::create(directory.path(), generation("safe")?)?;
    let sealed = builder.seal()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let target = directory.path().join("portable-source");
        fs::write(&target, "do not truncate")?;
        symlink(&target, directory.path().join("CURRENT.tmp"))?;
        let reader = sealed.activate()?;
        assert_eq!(reader.id().as_str(), "safe");
        assert_eq!(fs::read_to_string(target)?, "do not truncate");
        assert!(directory.path().join("CURRENT.tmp").is_symlink());
    }
    #[cfg(not(unix))]
    {
        let reader = sealed.activate()?;
        assert_eq!(reader.id().as_str(), "safe");
    }
    Ok(())
}

#[test]
fn activation_retries_precreated_current_temporary_names() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TestDirectory::new("temp-collisions")?;
    let sealed = GenerationBuilder::create(directory.path(), generation("safe")?)?.seal()?;
    let pid = std::process::id();
    for sequence in 0..256 {
        fs::write(
            directory
                .path()
                .join(format!("CURRENT.{pid}.{sequence}.tmp")),
            "unowned",
        )?;
    }

    let reader = sealed.activate()?;

    assert_eq!(reader.id().as_str(), "safe");
    for sequence in 0..256 {
        assert_eq!(
            fs::read_to_string(
                directory
                    .path()
                    .join(format!("CURRENT.{pid}.{sequence}.tmp"))
            )?,
            "unowned"
        );
    }
    Ok(())
}

#[test]
fn concurrent_activations_publish_and_return_their_own_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("concurrent-activation")?;
    let first = GenerationBuilder::create(directory.path(), generation("first")?)?.seal()?;
    let second = GenerationBuilder::create(directory.path(), generation("second")?)?.seal()?;
    let barrier = Arc::new(Barrier::new(3));
    let root = Arc::new(directory.path().to_owned());

    let first_thread = {
        let barrier = Arc::clone(&barrier);
        let root = Arc::clone(&root);
        thread::spawn(move || {
            barrier.wait();
            let reader = first.activate()?;
            assert_eq!(reader.id().as_str(), "first");
            assert_eq!(reader.directory(), root.join("first"));
            Ok::<_, IndexError>(())
        })
    };
    let second_thread = {
        let barrier = Arc::clone(&barrier);
        let root = Arc::clone(&root);
        thread::spawn(move || {
            barrier.wait();
            let reader = second.activate()?;
            assert_eq!(reader.id().as_str(), "second");
            assert_eq!(reader.directory(), root.join("second"));
            Ok::<_, IndexError>(())
        })
    };
    barrier.wait();
    first_thread
        .join()
        .map_err(|_| "first activation panicked")??;
    second_thread
        .join()
        .map_err(|_| "second activation panicked")??;

    let current = GenerationReader::open_current(directory.path())?;
    assert!(matches!(current.id().as_str(), "first" | "second"));
    let temporary_count = fs::read_dir(directory.path())?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("CURRENT."))
        .count();
    assert_eq!(temporary_count, 0);
    Ok(())
}

#[test]
fn corrupt_current_generation_reports_rebuild_without_deleting_anything()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("corrupt")?;
    publish(directory.path(), "corrupt", "repository")?;
    let graph = directory.path().join("corrupt").join("graph.db");
    fs::write(&graph, "not sqlite")?;
    let portable = directory.path().join("portable-source.jsonl");
    fs::write(&portable, "keep me")?;

    assert!(matches!(
        GenerationReader::open_current(directory.path()),
        Err(IndexError::RebuildRequired(_))
    ));
    assert!(directory.path().join("corrupt").exists());
    assert_eq!(fs::read_to_string(portable)?, "keep me");
    Ok(())
}

#[test]
fn validation_failure_never_replaces_the_last_valid_current()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("validation-fault")?;
    publish(directory.path(), "stable", "stable-repository")?;

    let builder = GenerationBuilder::create(directory.path(), generation("broken")?)?;
    let graph = rusqlite::Connection::open(builder.graph_path())?;
    graph.execute("DROP TABLE metadata", [])?;
    drop(graph);

    assert!(matches!(
        builder.seal(),
        Err(IndexError::RebuildRequired(_))
    ));
    let reader = GenerationReader::open_current(directory.path())?;
    assert_eq!(reader.id().as_str(), "stable");
    Ok(())
}

#[test]
fn a_reader_remains_pinned_after_current_switches() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("pinning")?;
    let old = publish(directory.path(), "old", "old-repository")?;
    let new = publish(directory.path(), "new", "new-repository")?;

    assert_eq!(old.id().as_str(), "old");
    assert_eq!(old.graph().counts()?.repositories(), 1);
    assert_eq!(new.id().as_str(), "new");
    assert_eq!(
        GenerationReader::open_current(directory.path())?.id(),
        new.id()
    );
    assert!(directory.path().join("old").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn generation_open_rejects_directory_and_graph_symlink_escapes()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let root = TestDirectory::new("generation-link-root")?;
    let outside = TestDirectory::new("generation-link-outside")?;
    publish(outside.path(), "outside", "outside-repository")?;

    symlink(outside.path(), root.path().join("linked-root"))?;
    assert!(matches!(
        GenerationReader::open(&root.path().join("linked-root"), generation("outside")?),
        Err(IndexError::RebuildRequired(_))
    ));

    symlink(outside.path().join("outside"), root.path().join("outside"))?;
    assert!(matches!(
        GenerationReader::open(root.path(), generation("outside")?),
        Err(IndexError::RebuildRequired(_))
    ));

    fs::create_dir(root.path().join("graph-link"))?;
    symlink(
        outside.path().join("outside/graph.db"),
        root.path().join("graph-link/graph.db"),
    )?;
    assert!(matches!(
        GenerationReader::open(root.path(), generation("graph-link")?),
        Err(IndexError::RebuildRequired(_))
    ));
    Ok(())
}

#[test]
fn generation_open_rejects_sqlite_sidecars() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("open-sidecars")?;
    let reader = publish(directory.path(), "sidecars", "repository")?;
    drop(reader);

    for suffix in ["-wal", "-shm"] {
        let sidecar = format!(
            "{}{suffix}",
            directory.path().join("sidecars/graph.db").display()
        );
        fs::write(&sidecar, b"sidecar")?;
        assert!(matches!(
            GenerationReader::open(directory.path(), generation("sidecars")?),
            Err(IndexError::RebuildRequired(_))
        ));
        fs::remove_file(sidecar)?;
    }
    Ok(())
}

fn publish(
    root: &std::path::Path,
    id: &str,
    repository_id: &str,
) -> Result<GenerationReader, Box<dyn std::error::Error>> {
    let builder = GenerationBuilder::create(root, generation(id)?)?;
    builder.graph().insert_repository(
        &RepositoryId::try_from(repository_id.to_owned())?,
        repository_id,
    )?;
    Ok(builder.seal()?.activate()?)
}

fn repository() -> Result<RepositoryId, pebble_core::error::DomainError> {
    RepositoryId::try_from("acme.pebble".to_owned())
}

fn generation(value: &str) -> Result<GenerationId, pebble_core::error::DomainError> {
    GenerationId::try_from(value.to_owned())
}

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-generation-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        let path = path.canonicalize()?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
