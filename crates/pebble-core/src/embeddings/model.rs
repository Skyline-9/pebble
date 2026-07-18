//! On-disk storage for installed local embedding models.
//!
//! Models live beneath `<state_root>/models/<model-id>/` as a `manifest.json`
//! alongside the manifest-declared weights, configuration, and tokenizer
//! files. [`ModelStore::install`] never fetches a byte itself; the caller
//! supplies `download_fn`, so production code injects a consent-gated
//! network fetch and tests inject a fake local source.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::EmbeddingError;
use super::manifest::{ManifestFile, ModelManifest, is_safe_component};

const MANIFEST_FILE: &str = "manifest.json";
const CURRENT_FILE: &str = "CURRENT";

/// On-disk store of installed local embedding models beneath
/// `<state_root>/models/`.
#[derive(Clone, Debug)]
pub struct ModelStore {
    models_root: PathBuf,
}

impl ModelStore {
    /// Open the model store rooted at `<state_root>/models`.
    #[must_use]
    pub fn new(state_root: &Path) -> Self {
        Self {
            models_root: state_root.join("models"),
        }
    }

    /// Install `manifest`, fetching each declared file through `download_fn`
    /// and rejecting the install if any file's SHA-256 digest does not match
    /// the manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when the manifest is invalid, a download fails, a
    /// checksum mismatches, or the install cannot be written to disk.
    pub fn install<F>(&self, manifest: &ModelManifest, download_fn: F) -> Result<(), EmbeddingError>
    where
        F: Fn(&str) -> io::Result<Vec<u8>>,
    {
        manifest.validate()?;
        let directory = self.model_dir(&manifest.id);
        fs::create_dir_all(&directory)?;
        fetch_and_store(&directory, &manifest.config, &download_fn)?;
        fetch_and_store(&directory, &manifest.weights, &download_fn)?;
        fetch_and_store(&directory, &manifest.tokenizer, &download_fn)?;
        let bytes = serde_json::to_vec_pretty(manifest)?;
        write_file_atomic(&directory.join(MANIFEST_FILE), &bytes)
    }

    /// List the manifests of every installed model.
    ///
    /// # Errors
    ///
    /// Returns an error when the models root or a stored manifest cannot be
    /// read or fails validation.
    pub fn list(&self) -> Result<Vec<ModelManifest>, EmbeddingError> {
        let mut manifests = Vec::new();
        let entries = match fs::read_dir(&self.models_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(manifests),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let manifest_path = entry.path().join(MANIFEST_FILE);
            if manifest_path.is_file() {
                manifests.push(read_manifest(&manifest_path)?);
            }
        }
        Ok(manifests)
    }

    /// Mark `model_id` as the active model for inference.
    ///
    /// # Errors
    ///
    /// Returns an error when `model_id` is not installed or the pointer file
    /// cannot be written.
    pub fn select(&self, model_id: &str) -> Result<(), EmbeddingError> {
        let directory = self.model_dir(model_id);
        if !is_safe_component(model_id) || !directory.join(MANIFEST_FILE).is_file() {
            return Err(EmbeddingError::ModelNotFound(model_id.to_owned()));
        }
        write_file_atomic(&self.models_root.join(CURRENT_FILE), model_id.as_bytes())
    }

    /// Return the manifest of the currently selected model, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when the current pointer's manifest exists but
    /// cannot be read or fails validation.
    pub fn current(&self) -> Result<Option<ModelManifest>, EmbeddingError> {
        let pointer = self.models_root.join(CURRENT_FILE);
        let model_id = match fs::read_to_string(&pointer) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let model_id = model_id.trim();
        if model_id.is_empty() || !is_safe_component(model_id) {
            return Ok(None);
        }
        let manifest_path = self.model_dir(model_id).join(MANIFEST_FILE);
        if !manifest_path.is_file() {
            return Ok(None);
        }
        Ok(Some(read_manifest(&manifest_path)?))
    }

    /// Remove an installed model.
    ///
    /// Switching the active model with [`ModelStore::select`] never deletes
    /// the previous model; callers must call `remove` explicitly. Clears the
    /// current-model pointer when it referenced the removed model.
    ///
    /// # Errors
    ///
    /// Returns an error when the model directory cannot be removed.
    pub fn remove(&self, model_id: &str) -> Result<(), EmbeddingError> {
        if !is_safe_component(model_id) {
            return Ok(());
        }
        match fs::remove_dir_all(self.model_dir(model_id)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.clear_current_if_matches(model_id)
    }

    fn clear_current_if_matches(&self, model_id: &str) -> Result<(), EmbeddingError> {
        let pointer = self.models_root.join(CURRENT_FILE);
        let Ok(current) = fs::read_to_string(&pointer) else {
            return Ok(());
        };
        if current.trim() == model_id {
            match fs::remove_file(&pointer) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn model_dir(&self, model_id: &str) -> PathBuf {
        self.models_root.join(model_id)
    }
}

fn fetch_and_store<F>(
    directory: &Path,
    file: &ManifestFile,
    download_fn: &F,
) -> Result<(), EmbeddingError>
where
    F: Fn(&str) -> io::Result<Vec<u8>>,
{
    let bytes = download_fn(&file.name)?;
    verify_checksum(&bytes, &file.sha256, &file.name)?;
    write_file_atomic(&directory.join(&file.name), &bytes)
}

fn verify_checksum(bytes: &[u8], expected: &str, name: &str) -> Result<(), EmbeddingError> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = hex_encode(hasher.finalize().as_ref());
    if actual == expected {
        Ok(())
    } else {
        Err(EmbeddingError::ChecksumMismatch {
            file: name.to_owned(),
            expected: expected.to_owned(),
            actual,
        })
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn read_manifest(path: &Path) -> Result<ModelManifest, EmbeddingError> {
    let bytes = fs::read(path)?;
    let manifest: ModelManifest = serde_json::from_slice(&bytes)?;
    manifest.validate()?;
    Ok(manifest)
}

/// Atomically write `bytes` to `path`, creating parent directories as needed.
pub(super) fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), EmbeddingError> {
    let Some(parent) = path.parent() else {
        return Err(EmbeddingError::InvalidManifest(
            "file path has no parent directory".to_owned(),
        ));
    };
    fs::create_dir_all(parent)?;
    let temp_path = temp_path(parent)?;
    let write_result = write_temp_file(&temp_path, bytes);
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

fn write_temp_file(path: &Path, bytes: &[u8]) -> Result<(), EmbeddingError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn temp_path(parent: &Path) -> Result<PathBuf, EmbeddingError> {
    for attempt in 0..1024u32 {
        let candidate = parent.join(format!(
            ".pebble-embeddings-{}-{attempt}.tmp",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(EmbeddingError::InvalidManifest(
        "unable to allocate a temporary file".to_owned(),
    ))
}
