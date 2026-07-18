//! Pinned local embedding model manifests.
//!
//! A manifest is the reproducible, checksum-verified description of one
//! optional local embedding model. Every field is fixed ahead of time so an
//! install is auditable before any byte leaves the network; Pebble never
//! infers manifest values from a download.

use serde::{Deserialize, Serialize};

use super::EmbeddingError;

/// Pooling strategy applied over token embeddings to build one sentence vector.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolingStrategy {
    /// Mean of token embeddings weighted by the attention mask.
    Mean,
    /// The first (`[CLS]`) token embedding.
    Cls,
}

/// One manifest-declared file and the checksum Pebble verifies before use.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManifestFile {
    /// File name relative to the model directory; never a path with separators.
    pub name: String,
    /// Lowercase hexadecimal SHA-256 digest of the file contents.
    pub sha256: String,
}

/// Pinned, checksum-verified description of one local embedding model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelManifest {
    /// Stable local identifier used as the on-disk directory name.
    pub id: String,
    /// Hugging Face repository id, e.g. `sentence-transformers/all-MiniLM-L6-v2`.
    pub repo_id: String,
    /// Pinned Git revision (commit hash) of the repository.
    pub revision: String,
    /// BERT-style `config.json` and its checksum.
    pub config: ManifestFile,
    /// Safetensors weights file and its checksum.
    pub weights: ManifestFile,
    /// Tokenizer JSON file and its checksum.
    pub tokenizer: ManifestFile,
    /// Output embedding dimensionality.
    pub dimensions: u32,
    /// Pooling strategy used to reduce token embeddings to one vector.
    pub pooling: PoolingStrategy,
    /// Whether output embeddings are L2-normalized.
    pub normalized: bool,
    /// SPDX license identifier declared by the model repository.
    pub license: String,
    /// Minimum Pebble runtime version required to run this model.
    pub min_runtime_version: String,
    /// Approximate installed size in bytes across all manifest files.
    pub approximate_install_bytes: u64,
    /// Approximate resident memory in bytes required during inference.
    pub approximate_ram_bytes: u64,
}

impl ModelManifest {
    /// The official recommended profile: a compact, general-purpose sentence
    /// embedding model (`sentence-transformers/all-MiniLM-L6-v2`).
    ///
    /// Files, revision, and checksums are pinned to the repository commit
    /// that added the `safetensors` weights variant, so an install is
    /// reproducible byte-for-byte.
    #[must_use]
    pub fn recommended() -> Self {
        Self {
            id: "all-minilm-l6-v2".to_owned(),
            repo_id: "sentence-transformers/all-MiniLM-L6-v2".to_owned(),
            revision: "46605decb5369335a3847c9f41bb0b896c07dd1a".to_owned(),
            config: ManifestFile {
                name: "config.json".to_owned(),
                sha256: "953f9c0d463486b10a6871cc2fd59f223b2c70184f49815e7efbcab5d8908b41"
                    .to_owned(),
            },
            weights: ManifestFile {
                name: "model.safetensors".to_owned(),
                sha256: "53aa51172d142c89d9012cce15ae4d6cc0ca6895895114379cacb4fab128d9db"
                    .to_owned(),
            },
            tokenizer: ManifestFile {
                name: "tokenizer.json".to_owned(),
                sha256: "be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037"
                    .to_owned(),
            },
            dimensions: 384,
            pooling: PoolingStrategy::Mean,
            normalized: true,
            license: "apache-2.0".to_owned(),
            min_runtime_version: "1.0.0".to_owned(),
            approximate_install_bytes: 91_335_235,
            approximate_ram_bytes: 150_000_000,
        }
    }

    /// Validate the structural invariants that must hold before install or load.
    ///
    /// # Errors
    ///
    /// Returns [`EmbeddingError::InvalidManifest`] when an identifier, file
    /// name, checksum, or dimension is malformed.
    pub fn validate(&self) -> Result<(), EmbeddingError> {
        if !is_safe_component(&self.id) {
            return Err(invalid("model id"));
        }
        if self.repo_id.is_empty() || self.revision.is_empty() {
            return Err(invalid("repository id or revision"));
        }
        if self.dimensions == 0 {
            return Err(invalid("embedding dimensions"));
        }
        if self.license.is_empty() || self.min_runtime_version.is_empty() {
            return Err(invalid("license or minimum runtime version"));
        }
        self.config.validate()?;
        self.weights.validate()?;
        self.tokenizer.validate()?;
        Ok(())
    }
}

impl ManifestFile {
    fn validate(&self) -> Result<(), EmbeddingError> {
        if !is_safe_component(&self.name) {
            return Err(invalid("manifest file name"));
        }
        if !is_lowercase_sha256(&self.sha256) {
            return Err(invalid("manifest file checksum"));
        }
        Ok(())
    }
}

/// Return whether `value` is safe to use as a single path component.
///
/// Rejects empty strings, path separators, and parent-directory references so
/// a manifest-declared name can never escape the model directory it belongs
/// to.
pub(super) fn is_safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn invalid(what: &'static str) -> EmbeddingError {
    EmbeddingError::InvalidManifest(what.to_owned())
}
