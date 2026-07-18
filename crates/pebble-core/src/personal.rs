//! Personal knowledge notes and promotion into a registered repository.
//!
//! Notes live under `<personal_root>/knowledge/*.md`; callers always pass
//! `personal_root` explicitly (normally `~/.pebble/v1/personal`) so tests can
//! use a temporary directory. Notes use the same frontmatter keys as the
//! shared knowledge-note format (`pebble_schema`, `pebble_id`, `title`), but
//! this module implements its own minimal reader and writer for the plain
//! case rather than depending on `crate::knowledge`; a later integration
//! pass should reconcile the duplication.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use thiserror::Error;
use ulid::Ulid;

#[path = "personal_promote.rs"]
mod promote;

pub use promote::{PromotionDiff, promote, promote_confirmed};

/// Failures creating, reading, or promoting personal knowledge notes.
#[derive(Debug, Error)]
pub enum PersonalError {
    /// A filesystem operation on personal-note storage failed.
    #[error("personal note storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// A note title was empty after trimming whitespace.
    #[error("personal note title must not be empty")]
    EmptyTitle,
    /// A note title contained a line break.
    #[error("personal note title {0:?} must not contain line breaks")]
    InvalidTitle(String),
    /// No personal note existed with the requested ID.
    #[error("personal note {0} was not found")]
    NotFound(String),
    /// A note's Markdown frontmatter was missing required fields.
    #[error("personal note frontmatter was malformed: {0}")]
    MalformedFrontmatter(String),
    /// A note ID contained characters unsafe for a filename.
    #[error("personal note ID {0:?} is not a portable identifier")]
    InvalidNoteId(String),
    /// A note title could not be reduced to a safe promotion slug.
    #[error("personal note title {0:?} cannot be promoted to a safe file name")]
    InvalidSlug(String),
    /// A computed promotion destination resolved outside the repository's
    /// knowledge directory.
    #[error("promotion destination escaped the repository knowledge directory")]
    PathTraversal,
    /// Promotion was attempted without explicit confirmation.
    #[error("personal note promotion requires explicit confirmation")]
    NotConfirmed,
    /// Promotion would silently overwrite different existing content.
    #[error("promotion would overwrite existing content at {}", .0.display())]
    WouldOverwrite(PathBuf),
}

/// One personal knowledge note loaded from or written to local storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalNote {
    id: String,
    title: String,
    content: String,
    path: PathBuf,
}

impl PersonalNote {
    /// Return the stable `pebble_id` frontmatter value, for example
    /// `note_01ARZ3NDEKTSV4RRFFQ69G5FAV`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Return the note's `title` frontmatter value.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Return the exact Markdown-plus-frontmatter file content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Return the local filesystem path this note was read from or written to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Storage operations over the personal-notes knowledge directory.
pub struct PersonalNoteStore;

impl PersonalNoteStore {
    /// Create a new personal note under `root` with a generated ULID-based ID.
    ///
    /// # Errors
    ///
    /// Returns an error if `title` is empty or contains a line break, or if
    /// the note file cannot be created.
    pub fn create(root: &Path, title: &str) -> Result<PersonalNote, PersonalError> {
        let title = validate_title(title)?;
        let directory = knowledge_dir(root);
        fs::create_dir_all(&directory)?;
        let id = format!("note_{}", Ulid::new());
        let content = render(&id, &title);
        let path = directory.join(format!("{id}.md"));
        write_new_file(&path, &content)?;
        Ok(PersonalNote {
            id,
            title,
            content,
            path,
        })
    }

    /// List every personal note stored under `root`, ordered by ID.
    ///
    /// Returns an empty list if the knowledge directory does not exist yet.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read or a note's
    /// frontmatter is malformed.
    pub fn list(root: &Path) -> Result<Vec<PersonalNote>, PersonalError> {
        let directory = knowledge_dir(root);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut notes = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) == Some("md") {
                notes.push(load_note(&path)?);
            }
        }
        notes.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(notes)
    }

    /// Read one personal note by its `pebble_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not a portable identifier, no note with
    /// that ID exists, or its frontmatter is malformed.
    pub fn read(root: &Path, id: &str) -> Result<PersonalNote, PersonalError> {
        validate_note_id(id)?;
        let path = knowledge_dir(root).join(format!("{id}.md"));
        if !path.is_file() {
            return Err(PersonalError::NotFound(id.to_owned()));
        }
        load_note(&path)
    }
}

fn knowledge_dir(root: &Path) -> PathBuf {
    root.join("knowledge")
}

fn validate_title(title: &str) -> Result<String, PersonalError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(PersonalError::EmptyTitle);
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(PersonalError::InvalidTitle(title.to_owned()));
    }
    Ok(trimmed.to_owned())
}

fn validate_note_id(id: &str) -> Result<(), PersonalError> {
    let portable = !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if portable {
        Ok(())
    } else {
        Err(PersonalError::InvalidNoteId(id.to_owned()))
    }
}

fn write_new_file(path: &Path, content: &str) -> Result<(), PersonalError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(content.as_bytes())?;
    file.flush()?;
    Ok(())
}

fn load_note(path: &Path) -> Result<PersonalNote, PersonalError> {
    let content = fs::read_to_string(path)?;
    let (id, title) = parse_frontmatter(&content)?;
    Ok(PersonalNote {
        id,
        title,
        content,
        path: path.to_owned(),
    })
}

fn render(id: &str, title: &str) -> String {
    format!(
        "---\npebble_schema: 1\npebble_id: {id}\ntitle: \"{escaped}\"\n---\n\n# {title}\n",
        escaped = escape_yaml_string(title)
    )
}

fn parse_frontmatter(content: &str) -> Result<(String, String), PersonalError> {
    let malformed = |reason: &str| PersonalError::MalformedFrontmatter(reason.to_owned());
    let after_open = content
        .strip_prefix("---\n")
        .ok_or_else(|| malformed("missing opening frontmatter fence"))?;
    let close = after_open
        .find("\n---\n")
        .ok_or_else(|| malformed("missing closing frontmatter fence"))?;
    let frontmatter = &after_open[..close];
    let mut id = None;
    let mut title = None;
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "pebble_id" => id = Some(unquote_yaml_string(value.trim())),
            "title" => title = Some(unquote_yaml_string(value.trim())),
            _ => {}
        }
    }
    let id = id.ok_or_else(|| malformed("missing pebble_id"))?;
    let title = title.ok_or_else(|| malformed("missing title"))?;
    Ok((id, title))
}

fn escape_yaml_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '\\' || character == '"' {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn unquote_yaml_string(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return value.to_owned();
    }
    let inner = &value[1..value.len() - 1];
    let mut unescaped = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            if let Some(escaped) = chars.next() {
                unescaped.push(escaped);
            }
        } else {
            unescaped.push(character);
        }
    }
    unescaped
}

#[cfg(test)]
mod tests {
    use super::{parse_frontmatter, render, unquote_yaml_string};

    #[test]
    fn render_and_parse_round_trip_plain_title() -> Result<(), Box<dyn std::error::Error>> {
        let content = render("note_TEST123", "Authentication notes");
        let (id, title) = parse_frontmatter(&content)?;
        assert_eq!(id, "note_TEST123");
        assert_eq!(title, "Authentication notes");
        Ok(())
    }

    #[test]
    fn render_and_parse_round_trip_quoted_title() -> Result<(), Box<dyn std::error::Error>> {
        let title = r#"Session: "expiry" & \backslash"#;
        let content = render("note_TEST456", title);
        let (_, parsed_title) = parse_frontmatter(&content)?;
        assert_eq!(parsed_title, title);
        assert_eq!(unquote_yaml_string("note_ABC"), "note_ABC");
        Ok(())
    }
}
