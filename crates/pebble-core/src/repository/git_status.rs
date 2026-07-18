//! Parsing for bounded porcelain Git status output.

use super::RepositoryError;
use super::identity::validate_git_path;

/// One changed repository path reported by porcelain Git status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedPath {
    pub(super) status: String,
    pub(super) fingerprint_status: String,
    pub(super) path: String,
    pub(super) previous_path: Option<String>,
}

impl ChangedPath {
    /// Return the primary one-letter status code.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }
    /// Return the current repository-relative slash path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Return the former path for a rename or copy.
    #[must_use]
    pub fn previous_path(&self) -> Option<&str> {
        self.previous_path.as_deref()
    }
}

pub(super) fn parse_status(bytes: &[u8]) -> Result<Vec<ChangedPath>, RepositoryError> {
    let records = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut changed = Vec::new();
    let mut index = 0;
    while index < records.len() && !records[index].is_empty() {
        let record = records[index];
        if record.len() < 4 || record[2] != b' ' {
            return Err(malformed());
        }
        let raw = &record[..2];
        let path = text(&record[3..])?;
        validate_git_path(path)?;
        let renamed = matches!(raw[0], b'R' | b'C') || matches!(raw[1], b'R' | b'C');
        let previous_path = if renamed {
            index += 1;
            let previous = records.get(index).ok_or_else(malformed)?;
            let previous = text(previous)?;
            validate_git_path(previous)?;
            Some(previous.to_owned())
        } else {
            None
        };
        let primary = if raw[0] == b' ' { raw[1] } else { raw[0] };
        changed.push(ChangedPath {
            status: char::from(primary).to_string(),
            fingerprint_status: text(raw)?.to_owned(),
            path: path.to_owned(),
            previous_path,
        });
        index += 1;
    }
    changed.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(changed)
}

fn text(bytes: &[u8]) -> Result<&str, RepositoryError> {
    std::str::from_utf8(bytes).map_err(|error| RepositoryError::InvalidGitOutput {
        operation: "status",
        message: error.to_string(),
    })
}

fn malformed() -> RepositoryError {
    RepositoryError::InvalidGitOutput {
        operation: "status",
        message: "malformed record".to_owned(),
    }
}
