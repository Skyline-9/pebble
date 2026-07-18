//! Consent-gated disclosure and per-model install approval.
//!
//! Pebble never downloads a model without first showing the disclosure
//! rendered by [`render_disclosure`] and recording explicit approval in a
//! [`ConsentState`](crate::embeddings::ConsentState). Nothing in this module
//! performs a download.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::EmbeddingError;
use super::manifest::ModelManifest;
use super::model::write_file_atomic;

const CONSENT_FILE: &str = "consent.json";
const MAX_CONSENT_BYTES: u64 = 64 * 1024;

/// Persisted consent state for optional local embedding models.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsentState {
    /// Whether the "no model installed" warning has already been shown once.
    #[serde(default)]
    pub warned_no_model: bool,
    /// Model IDs the user has explicitly approved for install.
    #[serde(default)]
    pub approved_model_ids: Vec<String>,
}

impl ConsentState {
    /// Load consent state from `<state_root>/models/consent.json`.
    ///
    /// Returns the default state (no warning shown, no approvals) when the
    /// file does not exist yet.
    ///
    /// # Errors
    ///
    /// Returns an error when the file exists but cannot be read or parsed, or
    /// exceeds the bounded consent-file size.
    pub fn load(state_root: &Path) -> Result<Self, EmbeddingError> {
        let bytes = match fs::read(Self::path(state_root)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
        };
        if bytes.len() as u64 > MAX_CONSENT_BYTES {
            return Err(EmbeddingError::InvalidManifest(
                "consent state exceeds its size bound".to_owned(),
            ));
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Persist consent state to `<state_root>/models/consent.json`.
    ///
    /// # Errors
    ///
    /// Returns an error when the state cannot be serialized or written.
    pub fn save(&self, state_root: &Path) -> Result<(), EmbeddingError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_file_atomic(&Self::path(state_root), &bytes)
    }

    /// Mark the "no model installed" warning as shown, returning the warning
    /// text only the first time this is called for a given consent state.
    #[must_use]
    pub fn warn_once_if_needed(&mut self) -> Option<String> {
        if self.warned_no_model {
            return None;
        }
        self.warned_no_model = true;
        Some(
            "Pebble has no local embedding model installed; semantic search is unavailable \
             and results are model-free. Run `pebble model install <model>` to enable it."
                .to_owned(),
        )
    }

    /// Whether the user has already approved installing `model_id`.
    #[must_use]
    pub fn has_install_consent(&self, model_id: &str) -> bool {
        self.approved_model_ids.iter().any(|id| id == model_id)
    }

    /// Record explicit approval to install `model_id`.
    pub fn grant_install_consent(&mut self, model_id: &str) {
        if !self.has_install_consent(model_id) {
            self.approved_model_ids.push(model_id.to_owned());
        }
    }

    fn path(state_root: &Path) -> PathBuf {
        state_root.join("models").join(CONSENT_FILE)
    }
}

/// Render the required pre-download disclosure for CLI/MCP display.
///
/// Shows the repository, pinned revision, every file Pebble will fetch with
/// its checksum, the approximate installed size, the estimated RAM use, and
/// the declared license. Pebble never downloads a model without first
/// showing this disclosure and receiving explicit approval.
#[must_use]
pub fn render_disclosure(manifest: &ModelManifest) -> String {
    format!(
        "Model: {}\n\
         Repository: {}\n\
         Revision: {}\n\
         Files:\n\
         \x20 - {} (sha256:{})\n\
         \x20 - {} (sha256:{})\n\
         \x20 - {} (sha256:{})\n\
         Approximate installed size: {} bytes\n\
         Estimated RAM use: {} bytes\n\
         License: {}\n",
        manifest.id,
        manifest.repo_id,
        manifest.revision,
        manifest.config.name,
        manifest.config.sha256,
        manifest.weights.name,
        manifest.weights.sha256,
        manifest.tokenizer.name,
        manifest.tokenizer.sha256,
        manifest.approximate_install_bytes,
        manifest.approximate_ram_bytes,
        manifest.license,
    )
}
