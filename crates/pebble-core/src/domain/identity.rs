//! Stable identities for repository and indexed entities.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::DomainError;

macro_rules! string_id {
    ($name:ident, $label:literal, $documentation:literal) => {
        #[doc = $documentation]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Borrow the stable string representation.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = DomainError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                validate_identifier(&value, $label)?;
                Ok(Self(value))
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::try_from(value).map_err(serde::de::Error::custom)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_id!(
    RepositoryId,
    "repository ID",
    "Canonical portable identity of a repository."
);
string_id!(
    GenerationId,
    "generation ID",
    "Identity of one immutable index generation."
);
string_id!(
    FileId,
    "file ID",
    "Deterministic identity of a source file."
);
string_id!(
    ChunkId,
    "chunk ID",
    "Deterministic identity of an indexed text chunk."
);
string_id!(
    SymbolId,
    "symbol ID",
    "Deterministic identity of a language symbol."
);

impl FileId {
    /// Derive a file identity from its repository and normalized relative path.
    #[must_use]
    pub fn derive(repository: &RepositoryId, path: &str) -> Self {
        Self(derive_id("file", &[repository.as_str(), path]))
    }
}

impl ChunkId {
    /// Derive a chunk identity with a stable ordinal within its source file.
    #[must_use]
    pub fn derive(
        file: &FileId,
        start_line: u32,
        end_line: u32,
        ordinal: usize,
        content_digest: &str,
    ) -> Self {
        let start_line = start_line.to_string();
        let end_line = end_line.to_string();
        let ordinal = ordinal.to_string();
        Self(derive_id(
            "chunk",
            &[
                file.as_str(),
                &start_line,
                &end_line,
                &ordinal,
                content_digest,
            ],
        ))
    }
}

impl SymbolId {
    /// Derive a symbol identity from its repository, language, and semantic name.
    #[must_use]
    pub fn derive(repository: &RepositoryId, language: &str, semantic_name: &str) -> Self {
        Self(derive_id(
            "symbol",
            &[repository.as_str(), language, semantic_name],
        ))
    }
}

fn validate_identifier(value: &str, kind: &'static str) -> Result<(), DomainError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(DomainError::InvalidIdentifier { kind });
    }
    Ok(())
}

fn derive_id(kind: &str, parts: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in std::iter::once(&kind).chain(parts) {
        let bytes = part.as_bytes();
        let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        hasher.update(&length.to_le_bytes());
        hasher.update(bytes);
    }
    format!("{kind}_{}", hasher.finalize().to_hex())
}
