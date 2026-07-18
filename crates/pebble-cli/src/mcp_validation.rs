//! Manual enforcement for every advertised MCP input-schema constraint.

use std::collections::BTreeSet;

use crate::mcp_tools::ToolError;

const MAX_PATH_BYTES: usize = 4_096;
const MAX_ID_BYTES: usize = 256;
const MAX_QUERY_BYTES: usize = 16_384;
const MAX_LANGUAGE_BYTES: usize = 64;
const MAX_WORKSPACE_NAME_BYTES: usize = 128;
const MAX_TITLE_BYTES: usize = 512;
const MAX_PATCH_BYTES: usize = 65_536;

pub fn path(value: &str) -> Result<(), ToolError> {
    string("repository", value, 1, MAX_PATH_BYTES)
}

pub fn id(field: &str, value: &str) -> Result<(), ToolError> {
    id_like(field, value, MAX_ID_BYTES)
}

pub fn trace(repository: &str, limit: usize) -> Result<(), ToolError> {
    id("repository", repository)?;
    if !(1..=1_000).contains(&limit) {
        return Err(invalid("limit must be between 1 and 1000"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn search(
    query: &str,
    repository: &str,
    budget_tokens: u32,
    max_results: usize,
    revision: Option<&str>,
    path_prefix: Option<&str>,
    language: Option<&str>,
    kinds: &[String],
) -> Result<(), ToolError> {
    string("query", query, 1, MAX_QUERY_BYTES)?;
    id("repository", repository)?;
    if !(1_000..=32_000).contains(&budget_tokens) {
        return Err(invalid("budget_tokens must be between 1000 and 32000"));
    }
    if !(1..=100).contains(&max_results) {
        return Err(invalid("max_results must be between 1 and 100"));
    }
    if let Some(value) = revision {
        string("revision", value, 1, MAX_ID_BYTES)?;
    }
    if let Some(value) = path_prefix {
        string("path_prefix", value, 1, MAX_PATH_BYTES)?;
    }
    if let Some(value) = language {
        string("language", value, 1, MAX_LANGUAGE_BYTES)?;
    }
    kinds_valid(kinds)
}

pub fn read(
    repository: &str,
    revision: &str,
    path: &str,
    start_line: u32,
    end_line: u32,
) -> Result<(), ToolError> {
    id("repository", repository)?;
    string("revision", revision, 1, MAX_ID_BYTES)?;
    string("path", path, 1, MAX_PATH_BYTES)?;
    if start_line == 0 || end_line == 0 {
        return Err(invalid("line numbers must be at least 1"));
    }
    Ok(())
}

pub fn workspace_name(value: &str) -> Result<(), ToolError> {
    id_like("name", value, MAX_WORKSPACE_NAME_BYTES)
}

pub fn model_install(model_id: &str) -> Result<(), ToolError> {
    id("model_id", model_id)
}

pub fn note_status(status: Option<&str>) -> Result<(), ToolError> {
    let Some(value) = status else {
        return Ok(());
    };
    if matches!(value, "current" | "stale" | "pending_update" | "broken") {
        Ok(())
    } else {
        Err(invalid("status is not a recognized claim status"))
    }
}

pub fn note_claim(repository: &str, claim_id: &str) -> Result<(), ToolError> {
    id("repository", repository)?;
    id("claim_id", claim_id)
}

pub fn update_apply(repository: &str, claim_id: &str, patch: &str) -> Result<(), ToolError> {
    id("repository", repository)?;
    id("claim_id", claim_id)?;
    string("patch", patch, 1, MAX_PATCH_BYTES)
}

pub fn workspace_add_repository(name: &str, repository: &str) -> Result<(), ToolError> {
    workspace_name(name)?;
    id("repository", repository)
}

pub fn workspace_search(
    name: &str,
    query: &str,
    budget_tokens: u32,
    max_results: usize,
) -> Result<(), ToolError> {
    workspace_name(name)?;
    string("query", query, 1, MAX_QUERY_BYTES)?;
    if !(1_000..=32_000).contains(&budget_tokens) {
        return Err(invalid("budget_tokens must be between 1000 and 32000"));
    }
    if !(1..=100).contains(&max_results) {
        return Err(invalid("max_results must be between 1 and 100"));
    }
    Ok(())
}

pub fn personal_title(title: &str) -> Result<(), ToolError> {
    string("title", title, 1, MAX_TITLE_BYTES)
}

pub fn personal_promote(note_id: &str, repository: &str) -> Result<(), ToolError> {
    id("note_id", note_id)?;
    id("repository", repository)
}

fn id_like(field: &str, value: &str, maximum: usize) -> Result<(), ToolError> {
    string(field, value, 1, maximum)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid(format!("{field} contains invalid characters")));
    }
    Ok(())
}

fn kinds_valid(kinds: &[String]) -> Result<(), ToolError> {
    if kinds.len() > 3 {
        return Err(invalid("kinds must contain at most 3 values"));
    }
    let mut seen = BTreeSet::new();
    for kind in kinds {
        if !matches!(kind.as_str(), "chunk" | "symbol" | "file") {
            return Err(invalid("kinds contains an unsupported value"));
        }
        if !seen.insert(kind) {
            return Err(invalid("kinds values must be unique"));
        }
    }
    Ok(())
}

fn string(field: &str, value: &str, minimum: usize, maximum: usize) -> Result<(), ToolError> {
    if !(minimum..=maximum).contains(&value.len()) {
        return Err(invalid(format!(
            "{field} must contain {minimum} through {maximum} UTF-8 bytes"
        )));
    }
    Ok(())
}

fn invalid(error: impl std::fmt::Display) -> ToolError {
    ToolError::Invalid(error.to_string())
}
