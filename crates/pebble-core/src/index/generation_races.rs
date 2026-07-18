//! Deterministic, test-only injection points for publication races.

use std::path::Path;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum RacePoint {
    BuildingCreated,
    LexicalDirectoryCreated,
    LexicalWriterCreated,
    LexicalCommit,
    LexicalReaderOpen,
    CurrentTemporary,
    CurrentPublished,
    GenerationPublished,
}

#[cfg(test)]
type Hook = Box<dyn FnOnce(&Path, &Path)>;

#[cfg(test)]
std::thread_local! {
    static INJECTION: std::cell::RefCell<Option<(RacePoint, Hook)>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn inject(point: RacePoint, hook: impl FnOnce(&Path, &Path) + 'static) {
    INJECTION.with(|injection| {
        *injection.borrow_mut() = Some((point, Box::new(hook)));
    });
}

#[cfg(test)]
pub(super) fn run(point: RacePoint, root: &Path, path: &Path) {
    INJECTION.with(|injection| {
        let mut injection = injection.borrow_mut();
        if injection
            .as_ref()
            .is_some_and(|(target, _)| *target == point)
            && let Some((_, hook)) = injection.take()
        {
            drop(injection);
            hook(root, path);
        }
    });
}

#[cfg(not(test))]
pub(super) const fn run(_point: RacePoint, _root: &Path, _path: &Path) {}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::domain::GenerationId;
    use crate::index::{GenerationBuilder, GenerationReader};

    use super::{RacePoint, inject};

    #[cfg(unix)]
    #[test]
    fn building_symlink_escape_is_rejected_before_lexical_creation()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new("building-symlink")?;
        let outside = TestRoot::new("building-symlink-outside")?;
        let outside_path = outside.path().to_owned();
        inject(RacePoint::BuildingCreated, move |_, building| {
            assert!(fs::remove_dir(building).is_ok());
            assert!(symlink(&outside_path, building).is_ok());
        });

        assert!(GenerationBuilder::create(root.path(), generation("candidate")?).is_err());
        assert!(!outside.path().join("lexical").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn lexical_symlink_escape_is_rejected_before_tantivy_creation()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new("lexical-symlink")?;
        let outside = TestRoot::new("lexical-symlink-outside")?;
        let outside_path = outside.path().to_owned();
        inject(RacePoint::LexicalDirectoryCreated, move |_, lexical| {
            assert!(fs::remove_dir(lexical).is_ok());
            assert!(symlink(&outside_path, lexical).is_ok());
        });

        assert!(GenerationBuilder::create(root.path(), generation("candidate")?).is_err());
        assert!(fs::read_dir(outside.path())?.next().is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn lexical_replacement_during_tantivy_creation_is_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new("lexical-replacement")?;
        let outside = TestRoot::new("lexical-replacement-outside")?;
        let outside_path = outside.path().to_owned();
        inject(RacePoint::LexicalWriterCreated, move |building, lexical| {
            assert!(fs::rename(lexical, building.join("original-lexical")).is_ok());
            assert!(symlink(&outside_path, lexical).is_ok());
        });

        assert!(GenerationBuilder::create(root.path(), generation("candidate")?).is_err());
        assert!(fs::read_dir(outside.path())?.next().is_none());
        Ok(())
    }

    #[test]
    fn replaced_current_temporary_restores_last_valid_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = TestRoot::new("temporary")?;
        publish(root.path(), "stable")?;
        let candidate = GenerationBuilder::create(root.path(), generation("candidate")?)?.seal()?;
        inject(RacePoint::CurrentTemporary, |_, path| {
            assert!(fs::remove_file(path).is_ok());
            assert!(fs::write(path, b"replacement\n").is_ok());
        });

        assert!(candidate.activate().is_err());
        assert_eq!(
            GenerationReader::open_current(root.path())?.id().as_str(),
            "stable"
        );
        Ok(())
    }

    #[test]
    fn replaced_published_current_restores_last_valid_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = TestRoot::new("published-current")?;
        publish(root.path(), "stable")?;
        let candidate = GenerationBuilder::create(root.path(), generation("candidate")?)?.seal()?;
        inject(RacePoint::CurrentPublished, |_, path| {
            assert!(fs::remove_file(path).is_ok());
            assert!(fs::write(path, b"replacement\n").is_ok());
        });

        assert!(candidate.activate().is_err());
        assert_eq!(
            GenerationReader::open_current(root.path())?.id().as_str(),
            "stable"
        );
        Ok(())
    }

    #[test]
    fn replaced_generation_after_publication_restores_last_valid_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = TestRoot::new("published-generation")?;
        publish(root.path(), "stable")?;
        let candidate = GenerationBuilder::create(root.path(), generation("candidate")?)?.seal()?;
        let replacement_root = TestRoot::new("replacement-generation")?;
        GenerationBuilder::create(replacement_root.path(), generation("candidate")?)?.seal()?;
        let replacement = replacement_root.path().join("candidate");
        inject(RacePoint::GenerationPublished, move |root, path| {
            assert!(fs::rename(path, root.join("candidate.replaced")).is_ok());
            assert!(fs::rename(replacement, path).is_ok());
        });

        assert!(candidate.activate().is_err());
        assert_eq!(
            GenerationReader::open_current(root.path())?.id().as_str(),
            "stable"
        );
        Ok(())
    }

    fn publish(root: &Path, id: &str) -> Result<(), Box<dyn std::error::Error>> {
        GenerationBuilder::create(root, generation(id)?)?
            .seal()?
            .activate()?;
        Ok(())
    }

    fn generation(value: &str) -> Result<GenerationId, crate::error::DomainError> {
        GenerationId::try_from(value.to_owned())
    }

    struct TestRoot(std::path::PathBuf);

    impl TestRoot {
        fn new(label: &str) -> std::io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "pebble-generation-race-{label}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path)?;
            let path = path.canonicalize()?;
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
}
