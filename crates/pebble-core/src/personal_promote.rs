//! Promotion of one personal note into a repository's shared, Git-owned
//! knowledge directory. Promotion never writes without explicit
//! confirmation, and never resolves a destination outside
//! `<target_repo_root>/.pebble/knowledge/`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::{PersonalError, PersonalNote};

/// The exact-byte diff of promoting one personal note into a repository,
/// computed without writing anything to disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromotionDiff {
    destination_path: PathBuf,
    unified_diff: String,
    would_overwrite: bool,
}

impl PromotionDiff {
    /// Return the path the note would be written to, for display.
    #[must_use]
    pub fn destination_path(&self) -> &Path {
        &self.destination_path
    }

    /// Return a unified diff between the current destination content (if
    /// any) and the personal note's content, for display before confirming.
    #[must_use]
    pub fn unified_diff(&self) -> &str {
        &self.unified_diff
    }

    /// Return whether writing this promotion would replace different
    /// existing content at the destination.
    #[must_use]
    pub const fn would_overwrite(&self) -> bool {
        self.would_overwrite
    }
}

/// Compute the diff of promoting `note` into `target_repo_root` without
/// writing anything to disk.
///
/// # Errors
///
/// Returns an error if the note's title cannot be reduced to a safe
/// filename slug, or if the existing destination content cannot be read.
pub fn promote(
    note: &PersonalNote,
    target_repo_root: &Path,
) -> Result<PromotionDiff, PersonalError> {
    let destination = destination_path(note, target_repo_root)?;
    let existing = read_existing(&destination)?;
    let would_overwrite =
        matches!(&existing, Some(existing) if existing.as_str() != note.content());
    let unified_diff = unified_diff(existing.as_deref(), note.content(), &destination);
    Ok(PromotionDiff {
        destination_path: destination,
        unified_diff,
        would_overwrite,
    })
}

/// Write `note` into `target_repo_root`'s shared knowledge directory.
///
/// This only writes when `confirmed` is `true`; it is a hard product
/// requirement that promotion never happens silently. When the destination
/// already exists with different content, the caller must additionally
/// pass `acknowledge_overwrite: true`, or the promotion is rejected.
///
/// # Errors
///
/// Returns an error if `confirmed` is `false`, if the destination exists
/// with different content and `acknowledge_overwrite` is `false`, if the
/// note's title cannot be reduced to a safe filename slug, or if the write
/// fails.
pub fn promote_confirmed(
    note: &PersonalNote,
    target_repo_root: &Path,
    confirmed: bool,
    acknowledge_overwrite: bool,
) -> Result<PathBuf, PersonalError> {
    if !confirmed {
        return Err(PersonalError::NotConfirmed);
    }
    let destination = destination_path(note, target_repo_root)?;
    let existing = read_existing(&destination)?;
    if let Some(existing) = &existing
        && existing.as_str() != note.content()
        && !acknowledge_overwrite
    {
        return Err(PersonalError::WouldOverwrite(destination));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&destination, note.content())?;
    Ok(destination)
}

fn destination_path(
    note: &PersonalNote,
    target_repo_root: &Path,
) -> Result<PathBuf, PersonalError> {
    let slug = slugify(note.title())?;
    let knowledge_dir = target_repo_root.join(".pebble").join("knowledge");
    let destination = knowledge_dir.join(format!("{slug}.md"));
    if destination.parent() == Some(knowledge_dir.as_path()) {
        Ok(destination)
    } else {
        Err(PersonalError::PathTraversal)
    }
}

/// Reduce a note title to a filesystem-safe slug.
///
/// Only ASCII letters, digits, spaces, `-`, and `_` are accepted; any other
/// character (including `.` and `/`) is rejected outright rather than
/// silently stripped, so a hostile title such as `../../etc/passwd` is
/// rejected instead of being sanitized into an unrelated file.
fn slugify(title: &str) -> Result<String, PersonalError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(PersonalError::InvalidSlug(title.to_owned()));
    }
    let mut slug = String::with_capacity(trimmed.len());
    for character in trimmed.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if matches!(character, ' ' | '-' | '_') {
            if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        } else {
            return Err(PersonalError::InvalidSlug(title.to_owned()));
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() {
        Err(PersonalError::InvalidSlug(title.to_owned()))
    } else {
        Ok(slug)
    }
}

fn read_existing(path: &Path) -> Result<Option<String>, PersonalError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

enum DiffOp<'a> {
    Equal(&'a str),
    Remove(&'a str),
    Add(&'a str),
}

fn unified_diff(old: Option<&str>, new: &str, destination: &Path) -> String {
    let new_lines: Vec<&str> = new.split('\n').collect();
    match old {
        None => {
            let ops: Vec<DiffOp<'_>> = new_lines.into_iter().map(DiffOp::Add).collect();
            render_unified(&ops, destination, true)
        }
        Some(old) if old == new => String::new(),
        Some(old) => {
            let old_lines: Vec<&str> = old.split('\n').collect();
            let ops = compute_diff(&old_lines, &new_lines);
            render_unified(&ops, destination, false)
        }
    }
}

fn compute_diff<'a>(old_lines: &[&'a str], new_lines: &[&'a str]) -> Vec<DiffOp<'a>> {
    let (rows, columns) = (old_lines.len(), new_lines.len());
    let mut lcs = vec![vec![0usize; columns + 1]; rows + 1];
    for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            lcs[row][column] = if old_lines[row] == new_lines[column] {
                lcs[row + 1][column + 1] + 1
            } else {
                lcs[row + 1][column].max(lcs[row][column + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(rows + columns);
    let (mut row, mut column) = (0, 0);
    while row < rows && column < columns {
        if old_lines[row] == new_lines[column] {
            ops.push(DiffOp::Equal(old_lines[row]));
            row += 1;
            column += 1;
        } else if lcs[row + 1][column] >= lcs[row][column + 1] {
            ops.push(DiffOp::Remove(old_lines[row]));
            row += 1;
        } else {
            ops.push(DiffOp::Add(new_lines[column]));
            column += 1;
        }
    }
    ops.extend(old_lines[row..].iter().copied().map(DiffOp::Remove));
    ops.extend(new_lines[column..].iter().copied().map(DiffOp::Add));
    ops
}

fn render_unified(ops: &[DiffOp<'_>], destination: &Path, new_file: bool) -> String {
    let old_count = ops
        .iter()
        .filter(|op| !matches!(op, DiffOp::Add(_)))
        .count();
    let new_count = ops
        .iter()
        .filter(|op| !matches!(op, DiffOp::Remove(_)))
        .count();
    let mut out = String::new();
    let from = if new_file {
        "/dev/null".to_owned()
    } else {
        format!("a/{}", destination.display())
    };
    let _ = writeln!(out, "--- {from}");
    let _ = writeln!(out, "+++ b/{}", destination.display());
    let _ = writeln!(out, "@@ -1,{old_count} +1,{new_count} @@");
    for op in ops {
        match op {
            DiffOp::Equal(line) => {
                let _ = writeln!(out, " {line}");
            }
            DiffOp::Remove(line) => {
                let _ = writeln!(out, "-{line}");
            }
            DiffOp::Add(line) => {
                let _ = writeln!(out, "+{line}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{destination_path, slugify, unified_diff};
    use crate::personal::{PersonalNote, PersonalNoteStore};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "pebble-personal-promote-{label}-{}-{suffix}",
            std::process::id()
        ))
    }

    fn note(root: &Path, title: &str) -> Result<PersonalNote, Box<dyn std::error::Error>> {
        Ok(PersonalNoteStore::create(root, title)?)
    }

    #[test]
    fn slugify_accepts_plain_titles() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(slugify("Authentication Notes")?, "authentication-notes");
        assert_eq!(slugify("  spaced   out  ")?, "spaced-out");
        Ok(())
    }

    #[test]
    fn slugify_rejects_path_traversal_titles() {
        assert!(slugify("../../etc/passwd").is_err());
        assert!(slugify("notes/with/slash").is_err());
        assert!(slugify("dotted.title").is_err());
        assert!(slugify("").is_err());
        assert!(slugify("   ").is_err());
    }

    #[test]
    fn destination_path_stays_inside_knowledge_directory() -> Result<(), Box<dyn std::error::Error>>
    {
        let repository = temp_dir("repository");
        let personal = temp_dir("personal");
        let created = note(&personal, "Safe Title")?;
        let destination = destination_path(&created, &repository)?;
        assert!(destination.starts_with(repository.join(".pebble").join("knowledge")));
        std::fs::remove_dir_all(&personal)?;
        Ok(())
    }

    #[test]
    fn unified_diff_is_empty_for_identical_content() {
        assert_eq!(unified_diff(Some("same"), "same", Path::new("x.md")), "");
    }

    #[test]
    fn unified_diff_marks_new_file_as_additions() {
        let diff = unified_diff(None, "line one\nline two", Path::new("x.md"));
        assert!(diff.contains("--- /dev/null"));
        assert!(diff.contains("+line one"));
        assert!(diff.contains("+line two"));
    }
}
