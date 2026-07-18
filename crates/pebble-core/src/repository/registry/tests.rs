use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::domain::RepositoryId;

use crate::repository::registry_race;

use super::RepositoryRegistry;

#[cfg(unix)]
#[test]
fn rejects_symbolic_link_registry() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let root = TestRoot::new("symlink")?;
    fs::write(root.path().join("target.json"), empty_registry())?;
    symlink(
        root.path().join("target.json"),
        root.path().join("registry.json"),
    )?;

    assert!(
        RepositoryRegistry::new(root.path())
            .registrations()
            .is_err()
    );
    Ok(())
}

#[test]
fn rejects_oversized_registry() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new("oversize")?;
    fs::write(root.path().join("registry.json"), vec![b' '; 1_048_577])?;

    assert!(
        RepositoryRegistry::new(root.path())
            .registrations()
            .is_err()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_nonregular_registry() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new("directory")?;
    fs::create_dir(root.path().join("registry.json"))?;

    assert!(
        RepositoryRegistry::new(root.path())
            .registrations()
            .is_err()
    );
    Ok(())
}

#[test]
fn rejects_registry_replaced_after_open() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new("replacement")?;
    let registry = root.path().join("registry.json");
    fs::write(&registry, empty_registry())?;
    let original = root.path().join("original.json");
    registry_race::inject(move |path| {
        assert!(fs::rename(path, original).is_ok());
        assert!(
            fs::write(
                path,
                br#"{"schema":1,"registrations":[{"repository_id":"evil","checkout":"/tmp","alternate_worktree":false}]}"#
            )
            .is_ok()
        );
    });

    assert!(
        RepositoryRegistry::new(root.path())
            .registrations()
            .is_err()
    );
    Ok(())
}

#[test]
fn rejects_registry_growth_after_open() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new("growth")?;
    let registry = root.path().join("registry.json");
    fs::write(&registry, empty_registry())?;
    registry_race::inject(|path| {
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|file| file.set_len(1_048_577))
                .is_ok()
        );
    });

    assert!(
        RepositoryRegistry::new(root.path())
            .registrations()
            .is_err()
    );
    Ok(())
}

#[test]
fn valid_registry_still_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    let state = TestRoot::new("valid")?;
    let checkout = TestRoot::new("checkout")?;
    let id = RepositoryId::try_from("acme.repo".to_owned())?;
    let registry = RepositoryRegistry::new(state.path());

    registry.register(&id, checkout.path(), false)?;

    assert_eq!(registry.registrations()?.len(), 1);
    Ok(())
}

fn empty_registry() -> &'static [u8] {
    br#"{"schema":1,"registrations":[]}"#
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "pebble-registry-read-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
