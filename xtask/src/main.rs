#![forbid(unsafe_code)]

//! Repository-local validation commands.

mod dependency_records;
mod file_size;

use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let command = arguments.next();
    let root = arguments.next();
    if arguments.next().is_some() {
        return usage();
    }
    let root = root.as_deref().map_or_else(|| Path::new("."), Path::new);
    match command.as_deref() {
        Some("check-file-size") => check_file_size(root),
        Some("check-dependency-records") => check_dependency_records(root),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: cargo run -p xtask -- \
         <check-file-size|check-dependency-records> [root]"
    );
    ExitCode::from(2)
}

fn check_file_size(root: &Path) -> ExitCode {
    match file_size::check(root) {
        Ok(violations) if violations.is_empty() => ExitCode::SUCCESS,
        Ok(violations) => {
            for violation in violations {
                eprintln!(
                    "{}: {} lines exceeds {}",
                    violation.path.display(),
                    violation.lines,
                    violation.limit
                );
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("file-size check failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn check_dependency_records(root: &Path) -> ExitCode {
    match dependency_records::check_repository(root) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dependency decision check failed: {error}");
            ExitCode::FAILURE
        }
    }
}
