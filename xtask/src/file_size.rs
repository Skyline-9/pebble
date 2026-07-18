#![allow(dead_code, reason = "shared by the xtask binary and integration tests")]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One Rust source file that exceeds its configured physical-line limit.
pub struct Violation {
    /// Repository-relative or absolute path reported by the scanner.
    pub path: PathBuf,
    /// Physical line count.
    pub lines: usize,
    /// Maximum allowed physical lines for this file class.
    pub limit: usize,
}

/// Find oversized Rust files beneath `root`.
///
/// # Errors
///
/// Returns an I/O error when a directory or Rust source file cannot be read.
pub fn check(root: &Path) -> io::Result<Vec<Violation>> {
    let mut rust_files = Vec::new();
    collect_rust_files(root, &mut rust_files)?;
    let mut violations = Vec::new();

    for path in rust_files {
        let contents = fs::read_to_string(&path)?;
        let lines = contents.lines().count();
        let limit = if is_test_or_benchmark(&path) {
            500
        } else {
            300
        };
        if lines > limit {
            violations.push(Violation { path, lines, limit });
        }
    }

    violations.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(violations)
}

fn collect_rust_files(path: &Path, output: &mut Vec<PathBuf>) -> io::Result<()> {
    if ignored(path) {
        return Ok(());
    }

    if path.is_file() {
        if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path.to_path_buf());
        }
        return Ok(());
    }

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_rust_files(&entry?.path(), output)?;
        }
    }

    Ok(())
}

fn ignored(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | "node_modules" | "target" | ".superpowers")
        )
    })
}

fn is_test_or_benchmark(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component.as_os_str().to_str(), Some("tests" | "benches")))
}
