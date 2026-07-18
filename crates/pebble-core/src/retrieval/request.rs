//! Validated retrieval request and metadata scope.

use std::path::{Component, Path, PathBuf};

use crate::domain::{
    DEFAULT_EVIDENCE_TOKENS, MAX_EVIDENCE_TOKENS, MIN_EVIDENCE_TOKENS, RepositoryId,
};

use super::{RetrievalError, trace};

const MAX_QUERY_BYTES: usize = 16 * 1024;
const DEFAULT_MAX_RESULTS: usize = 10;
const MAX_RESULTS: usize = 100;

/// Validated model-free search input and metadata scope.
#[derive(Clone, Debug)]
pub struct SearchRequest {
    query: String,
    budget_tokens: u32,
    max_results: usize,
    repository: Option<String>,
    revision: Option<String>,
    path_prefix: Option<String>,
    language: Option<String>,
    kinds: Vec<String>,
    pub(super) trace_path: Option<PathBuf>,
}

impl SearchRequest {
    /// Construct a search with the 6,000-token default evidence budget.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, oversized, or control-character input.
    pub fn new(query: impl Into<String>) -> Result<Self, RetrievalError> {
        let query = query.into();
        if query.trim().is_empty()
            || query.len() > MAX_QUERY_BYTES
            || query.chars().any(char::is_control)
        {
            return Err(RetrievalError::InvalidRequest(
                "query must contain 1 through 16384 printable UTF-8 bytes".to_owned(),
            ));
        }
        Ok(Self {
            query,
            budget_tokens: DEFAULT_EVIDENCE_TOKENS,
            max_results: DEFAULT_MAX_RESULTS,
            repository: None,
            revision: None,
            path_prefix: None,
            language: None,
            kinds: Vec::new(),
            trace_path: None,
        })
    }

    /// Set the inclusive 1,000 through 32,000 token evidence budget.
    ///
    /// # Errors
    ///
    /// Returns an error outside the supported budget range.
    pub fn with_budget_tokens(mut self, budget_tokens: u32) -> Result<Self, RetrievalError> {
        if !(MIN_EVIDENCE_TOKENS..=MAX_EVIDENCE_TOKENS).contains(&budget_tokens) {
            return Err(RetrievalError::InvalidRequest(format!(
                "evidence budget must be between {MIN_EVIDENCE_TOKENS} and {MAX_EVIDENCE_TOKENS}"
            )));
        }
        self.budget_tokens = budget_tokens;
        Ok(self)
    }

    /// Set the bounded number of packet items.
    ///
    /// # Errors
    ///
    /// Returns an error unless the limit is between 1 and 100.
    pub fn with_max_results(mut self, max_results: usize) -> Result<Self, RetrievalError> {
        if !(1..=MAX_RESULTS).contains(&max_results) {
            return Err(RetrievalError::InvalidRequest(
                "result limit must be between 1 and 100".to_owned(),
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Restrict results to one validated repository ID.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid repository ID.
    pub fn with_repository(
        mut self,
        repository: impl Into<String>,
    ) -> Result<Self, RetrievalError> {
        let repository = RepositoryId::try_from(repository.into())?;
        self.repository = Some(repository.as_str().to_owned());
        Ok(self)
    }

    /// Restrict results to one exact indexed revision string.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, oversized, or non-hex revision text.
    pub fn with_revision(mut self, revision: impl Into<String>) -> Result<Self, RetrievalError> {
        let revision = revision.into();
        validate_revision(&revision)?;
        self.revision = Some(revision);
        Ok(self)
    }

    /// Restrict results to a normalized repository-relative path prefix.
    ///
    /// # Errors
    ///
    /// Returns an error for an absolute, backslash, or parent-traversing path.
    pub fn with_path_prefix(mut self, path: impl Into<String>) -> Result<Self, RetrievalError> {
        let path = path.into();
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || Path::new(&path)
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        {
            return Err(RetrievalError::InvalidRequest(
                "path prefix must be normalized and repository-relative".to_owned(),
            ));
        }
        self.path_prefix = Some(path);
        Ok(self)
    }

    /// Restrict results to one lowercase language name.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty, oversized, or malformed language.
    pub fn with_language(mut self, language: impl Into<String>) -> Result<Self, RetrievalError> {
        let language = language.into();
        if language.is_empty()
            || language.len() > 64
            || !language
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(RetrievalError::InvalidRequest(
                "language must be a lowercase name".to_owned(),
            ));
        }
        self.language = Some(language);
        Ok(self)
    }

    /// Restrict results to chunk, symbol, or file entity kinds.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported kind.
    pub fn with_kinds<I, S>(mut self, kinds: I) -> Result<Self, RetrievalError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut values = kinds.into_iter().map(Into::into).collect::<Vec<_>>();
        values.sort();
        values.dedup();
        if values.is_empty()
            || values
                .iter()
                .any(|value| !matches!(value.as_str(), "chunk" | "symbol" | "file"))
        {
            return Err(RetrievalError::InvalidRequest(
                "entity kinds must be chunk, symbol, or file".to_owned(),
            ));
        }
        self.kinds = values;
        Ok(self)
    }

    /// Append this query's bounded record to a secure local JSONL path.
    ///
    /// # Errors
    ///
    /// Returns an error for relative, non-JSONL, parent-traversing, or
    /// symlinked paths.
    pub fn with_trace_path(mut self, path: &Path) -> Result<Self, RetrievalError> {
        self.trace_path = Some(trace::validate_path(path)?);
        Ok(self)
    }

    /// Return the lexical query text.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Return the evidence budget.
    #[must_use]
    pub const fn budget_tokens(&self) -> u32 {
        self.budget_tokens
    }

    /// Return the packet item ceiling.
    #[must_use]
    pub const fn max_results(&self) -> usize {
        self.max_results
    }

    pub(super) fn matches_resolved(&self, entity: &crate::index::RetrievalEntity) -> bool {
        self.matches_metadata(
            &entity.repository,
            &entity.revision,
            &entity.path,
            &entity.language,
            &entity.kind,
        )
    }

    fn matches_metadata(
        &self,
        repository: &str,
        revision: &str,
        path: &str,
        language: &str,
        kind: &str,
    ) -> bool {
        self.repository
            .as_deref()
            .is_none_or(|value| value == repository)
            && self
                .revision
                .as_deref()
                .is_none_or(|value| value == revision)
            && self
                .path_prefix
                .as_deref()
                .is_none_or(|value| path.starts_with(value))
            && self
                .language
                .as_deref()
                .is_none_or(|value| value == language)
            && (self.kinds.is_empty() || self.kinds.iter().any(|value| value == kind))
    }
}

fn validate_revision(value: &str) -> Result<(), RetrievalError> {
    let (base, dirty) = value
        .split_once("+dirty.")
        .map_or((value, None), |(base, dirty)| (base, Some(dirty)));
    let invalid_hex = |part: &str| {
        part.is_empty()
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    };
    if invalid_hex(base) || dirty.is_some_and(invalid_hex) {
        return Err(RetrievalError::InvalidRequest(
            "revision must be lowercase hexadecimal with an optional dirty digest".to_owned(),
        ));
    }
    Ok(())
}
