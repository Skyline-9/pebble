use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::service::trace_race;

use super::read_traces;

#[cfg(unix)]
#[test]
fn rejects_trace_symbolic_link_and_special_file() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let root = TestRoot::new("unsafe")?;
    let target = root.path().join("target.jsonl");
    fs::write(&target, b"")?;
    let link = root.path().join("link.jsonl");
    symlink(target, &link)?;

    assert!(read_traces(&link, 1).is_err());
    assert!(read_traces(Path::new("/dev/zero"), 1).is_err());
    Ok(())
}

#[test]
fn rejects_trace_replaced_after_open() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new("replacement")?;
    let trace = root.path().join("traces.jsonl");
    fs::write(&trace, b"")?;
    let displaced = root.path().join("original.jsonl");
    trace_race::inject(move |path| {
        assert!(fs::rename(path, &displaced).is_ok());
        assert!(fs::write(path, b"").is_ok());
    });

    assert!(read_traces(&trace, 1).is_err());
    Ok(())
}

#[test]
fn rejects_trace_growth_after_open() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new("growth")?;
    let trace = root.path().join("traces.jsonl");
    fs::write(&trace, b"")?;
    trace_race::inject(|path| {
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|file| file.set_len(8 * 1024 * 1024 + 1))
                .is_ok()
        );
    });

    assert!(read_traces(&trace, 1).is_err());
    Ok(())
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "pebble-trace-read-{label}-{}-{}",
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
