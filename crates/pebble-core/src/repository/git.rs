//! Restricted, bounded system-Git subprocess boundary.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::git_status::{ChangedPath, parse_status};
use super::identity::{hash_part, hash_worktree, validate_git_path};
use super::{GitOutput, RepositoryError, bounded_output, git_executable};
use crate::domain::WorktreeRevision;

const DEFAULT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Discovered system Git executable with fixed resource limits.
#[derive(Clone, Debug)]
pub struct SystemGit {
    executable: PathBuf,
    timeout: Duration,
    output_limit: usize,
}

impl SystemGit {
    /// Discover `git` on the process search path.
    /// # Errors
    /// Returns an error when no executable named `git` is present.
    pub fn discover() -> Result<Self, RepositoryError> {
        let path = std::env::var_os("PATH").ok_or(RepositoryError::GitNotFound)?;
        Self::discover_in(Path::new(&path))
    }
    /// Discover `git` in an explicit search path.
    /// # Errors
    /// Returns an error when no executable named `git` is present.
    pub fn discover_in(search_path: &Path) -> Result<Self, RepositoryError> {
        Self::discover_in_with_limits(search_path, DEFAULT_TIMEOUT, DEFAULT_OUTPUT_LIMIT)
    }
    /// Discover `git` with explicit completion and output bounds.
    /// # Errors
    /// Returns an error when `git` is missing or a limit is zero.
    pub fn discover_in_with_limits(
        search_path: &Path,
        timeout: Duration,
        output_limit: usize,
    ) -> Result<Self, RepositoryError> {
        if timeout.is_zero() || output_limit == 0 {
            return Err(RepositoryError::InvalidConfig(
                "Git limits must be nonzero".to_owned(),
            ));
        }
        let executable = std::env::split_paths(search_path)
            .map(|directory| directory.join(executable_name()))
            .find(|candidate| git_executable(candidate))
            .ok_or(RepositoryError::GitNotFound)?;
        Ok(Self {
            executable,
            timeout,
            output_limit,
        })
    }
    /// Read the canonical `origin` remote without consulting included config.
    /// # Errors
    /// Returns an error when Git cannot inspect the repository.
    pub fn origin_remote(&self, repository: &Path) -> Result<Option<String>, RepositoryError> {
        let output = self.run(
            repository,
            "remote discovery",
            &[
                "config",
                "--local",
                "--no-includes",
                "--get",
                "remote.origin.url",
            ],
        )?;
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        if !output.status.success() {
            return Err(RepositoryError::GitFailed {
                operation: "remote discovery",
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        let remote = text(&output.stdout, "remote discovery")?.trim().to_owned();
        Ok((!remote.is_empty()).then_some(remote))
    }
    /// Compute the clean base OID and optional dirty snapshot digest.
    /// # Errors
    /// Returns an error for Git, validation, limit, or filesystem failures.
    pub fn revision(&self, repository: &Path) -> Result<WorktreeRevision, RepositoryError> {
        let base = self.success(
            repository,
            "revision",
            &["rev-parse", "--verify", "HEAD^{commit}"],
        )?;
        let base = text(&base, "revision")?.trim();
        let changed = self.changed_paths(repository)?;
        if changed.is_empty() {
            return WorktreeRevision::clean(base).map_err(Into::into);
        }
        let index = self.index_oids(repository, &changed)?;
        let mut hasher = blake3::Hasher::new();
        hash_part(&mut hasher, base.as_bytes());
        for item in &changed {
            hash_part(&mut hasher, item.fingerprint_status.as_bytes());
            hash_part(
                &mut hasher,
                item.previous_path.as_deref().unwrap_or("").as_bytes(),
            );
            hash_part(&mut hasher, item.path.as_bytes());
            match index.get(&item.path).map(Vec::as_slice) {
                Some([(0, oid)]) => hash_part(&mut hasher, oid.as_bytes()),
                Some(stages) => {
                    for (stage, oid) in stages {
                        hash_part(&mut hasher, &[*stage]);
                        hash_part(&mut hasher, oid.as_bytes());
                    }
                }
                None => hash_part(&mut hasher, b""),
            }
            hash_worktree(&mut hasher, repository, &item.path)?;
        }
        WorktreeRevision::dirty(base, hasher.finalize().to_hex().to_string()).map_err(Into::into)
    }
    /// Return deterministic changed paths, including rename source paths.
    /// # Errors
    /// Returns an error for Git, validation, or resource-limit failures.
    pub fn changed_paths(&self, repository: &Path) -> Result<Vec<ChangedPath>, RepositoryError> {
        let bytes = self.success(
            repository,
            "status",
            &[
                "-c",
                "status.renames=true",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--",
            ],
        )?;
        parse_status(&bytes)
    }

    fn index_oids(
        &self,
        repository: &Path,
        changed: &[ChangedPath],
    ) -> Result<BTreeMap<String, Vec<(u8, String)>>, RepositoryError> {
        let mut arguments = vec!["ls-files", "--stage", "-z", "--"];
        arguments.extend(changed.iter().map(|item| item.path.as_str()));
        let bytes = self.success(repository, "index", &arguments)?;
        let mut entries = BTreeMap::new();
        for record in bytes
            .split(|byte| *byte == 0)
            .filter(|item| !item.is_empty())
        {
            let record = text(record, "index")?;
            let (metadata, path) = record.split_once('\t').ok_or_else(|| malformed("index"))?;
            let fields = metadata.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(malformed("index"));
            }
            validate_git_path(path)?;
            let stage = fields[2].parse::<u8>().map_err(|_| malformed("index"))?;
            if stage > 3 {
                return Err(malformed("index"));
            }
            entries
                .entry(path.to_owned())
                .or_insert_with(Vec::new)
                .push((stage, fields[1].to_owned()));
        }
        for stages in entries.values_mut() {
            stages.sort();
        }
        Ok(entries)
    }

    fn success(
        &self,
        repository: &Path,
        operation: &'static str,
        arguments: &[&str],
    ) -> Result<Vec<u8>, RepositoryError> {
        let output = self.run(repository, operation, arguments)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(RepositoryError::GitFailed {
                operation,
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    fn run(
        &self,
        repository: &Path,
        operation: &'static str,
        arguments: &[&str],
    ) -> Result<GitOutput, RepositoryError> {
        let mut command = Command::new(&self.executable);
        command
            .args([
                "--no-optional-locks",
                "--literal-pathspecs",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.excludesFile=",
                "-C",
            ])
            .arg(repository)
            .arg("--work-tree=.")
            .args(arguments)
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        bounded_output(command, operation, self.timeout, self.output_limit)
    }
}

fn text<'a>(bytes: &'a [u8], operation: &'static str) -> Result<&'a str, RepositoryError> {
    std::str::from_utf8(bytes).map_err(|error| RepositoryError::InvalidGitOutput {
        operation,
        message: error.to_string(),
    })
}

fn malformed(operation: &'static str) -> RepositoryError {
    RepositoryError::InvalidGitOutput {
        operation,
        message: "malformed record".to_owned(),
    }
}

const fn executable_name() -> &'static str {
    if cfg!(windows) { "git.exe" } else { "git" }
}
