//! Consent-gated local embedding model install/select and model-augmented search.

use std::io;

use serde::Serialize;

use crate::domain::RepositoryId;
use crate::embeddings::{
    ConsentState, EmbeddingError, EmbeddingRuntime, ModelManifest, ModelStore,
};
use crate::index::GenerationReader;
use crate::retrieval::{QueryEmbedding, RetrievalEngine, SearchRequest, SearchResponse};
use crate::vectors::FINGERPRINT_LEN;

use super::health::{map_index_unavailable, map_retrieval};
use super::{PebbleService, ServiceError};

/// Largest single manifest file Pebble will download from Hugging Face.
const MAX_MODEL_FILE_BYTES: u64 = 200 * 1024 * 1024;

/// Result of a consent-checked embedding model install request.
#[derive(Clone, Debug, Serialize)]
pub struct ModelInstallResult {
    /// Stable identifier of the requested model.
    pub model_id: String,
    /// Whether the model was actually downloaded and installed.
    pub installed: bool,
    /// Required pre-download disclosure text, present only when `confirm`
    /// was not set and nothing was installed.
    pub disclosure: Option<String>,
    /// Installed model manifest, present only once installed.
    pub manifest: Option<ModelManifest>,
}

/// Result of removing one installed embedding model.
#[derive(Clone, Debug, Serialize)]
pub struct ModelRemoval {
    /// Stable identifier of the removed model.
    pub model_id: String,
}

impl PebbleService {
    /// Show the install disclosure for `model_id`, or install it when
    /// `confirm` is `true`.
    ///
    /// Pebble never downloads a model without first showing this disclosure;
    /// callers must call this once with `confirm: false` and then again
    /// with `confirm: true` to proceed.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error for an unknown model ID, or an
    /// operational error when the download, checksum verification, or local
    /// install fails.
    pub fn model_install(
        &self,
        model_id: &str,
        confirm: bool,
    ) -> Result<ModelInstallResult, ServiceError> {
        let manifest = resolve_manifest(model_id)?;
        if !confirm {
            return Ok(ModelInstallResult {
                model_id: manifest.id.clone(),
                installed: false,
                disclosure: Some(crate::embeddings::render_disclosure(&manifest)),
                manifest: None,
            });
        }
        let mut consent = ConsentState::load(self.state_root()).map_err(map_embedding)?;
        consent.grant_install_consent(&manifest.id);
        consent.save(self.state_root()).map_err(map_embedding)?;
        ModelStore::new(self.state_root())
            .install(&manifest, |file_name| {
                download_model_file(&manifest, file_name)
            })
            .map_err(map_embedding)?;
        Ok(ModelInstallResult {
            model_id: manifest.id.clone(),
            installed: true,
            disclosure: None,
            manifest: Some(manifest),
        })
    }

    /// List every installed embedding model's manifest.
    ///
    /// # Errors
    ///
    /// Returns an operational error when the model store cannot be read.
    pub fn model_list(&self) -> Result<Vec<ModelManifest>, ServiceError> {
        ModelStore::new(self.state_root())
            .list()
            .map_err(map_embedding)
    }

    /// Select `model_id` as the active model for embedding-augmented search.
    ///
    /// # Errors
    ///
    /// Returns a classified usage error when `model_id` is not installed.
    pub fn model_select(&self, model_id: &str) -> Result<ModelManifest, ServiceError> {
        let store = ModelStore::new(self.state_root());
        store.select(model_id).map_err(map_embedding)?;
        store
            .current()
            .map_err(map_embedding)?
            .ok_or_else(|| ServiceError::operational("selected model manifest is unavailable"))
    }

    /// Remove one installed embedding model, clearing the active selection
    /// when it referenced the removed model.
    ///
    /// # Errors
    ///
    /// Returns an operational error when the model directory cannot be removed.
    pub fn model_remove(&self, model_id: &str) -> Result<ModelRemoval, ServiceError> {
        ModelStore::new(self.state_root())
            .remove(model_id)
            .map_err(map_embedding)?;
        Ok(ModelRemoval {
            model_id: model_id.to_owned(),
        })
    }

    /// Search one pinned immutable generation, optionally folding in
    /// cosine-similarity vector candidates from the currently selected
    /// embedding model.
    ///
    /// When `embed_query` is `false`, or no model is currently selected,
    /// this method behaves exactly like [`PebbleService::search`].
    ///
    /// # Errors
    ///
    /// Returns the same classified errors as [`PebbleService::search`], plus
    /// an operational error when the selected model cannot be loaded or run.
    pub fn search_with_model(
        &self,
        repository: &RepositoryId,
        request: SearchRequest,
        embed_query: bool,
    ) -> Result<SearchResponse, ServiceError> {
        if !embed_query {
            return self.search(repository, request);
        }
        let store = ModelStore::new(self.state_root());
        let Some(manifest) = store.current().map_err(map_embedding)? else {
            return self.search(repository, request);
        };
        self.registration(repository)?;
        let reader = GenerationReader::open_current(&self.layout.generations(repository))
            .map_err(map_index_unavailable)?;
        let model_dir = self.state_root().join("models").join(&manifest.id);
        let runtime = EmbeddingRuntime::load(&model_dir).map_err(map_embedding)?;
        let query_text = request.query().to_owned();
        let request = request
            .with_trace_path(&self.repository_root(repository).join("traces.jsonl"))
            .map_err(map_retrieval)?;
        let embedding =
            embed_query_text(&runtime, &query_text, &manifest).map_err(map_embedding)?;
        RetrievalEngine::new(&reader)
            .search_with_embedding(request, embedding.as_ref())
            .map_err(map_retrieval)
    }
}

fn embed_query_text(
    runtime: &EmbeddingRuntime,
    query_text: &str,
    manifest: &ModelManifest,
) -> Result<Option<QueryEmbedding>, EmbeddingError> {
    let vectors = runtime.embed(&[query_text])?;
    let Some(vector) = vectors.into_iter().next() else {
        return Ok(None);
    };
    Ok(QueryEmbedding::new(vector, model_fingerprint(manifest)).ok())
}

/// Derive a stable fingerprint identifying `manifest`'s exact pinned model
/// and configuration, matching the fingerprint a compile-time embedder for
/// the same model would report.
fn model_fingerprint(manifest: &ModelManifest) -> [u8; FINGERPRINT_LEN] {
    let mut hasher = blake3::Hasher::new();
    for part in [
        manifest.repo_id.as_str(),
        manifest.revision.as_str(),
        manifest.weights.sha256.as_str(),
        manifest.tokenizer.sha256.as_str(),
        manifest.config.sha256.as_str(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update(&[0]);
    }
    hasher.update(&manifest.dimensions.to_le_bytes());
    hasher.update(&[u8::from(manifest.normalized)]);
    *hasher.finalize().as_bytes()
}

fn resolve_manifest(model_id: &str) -> Result<ModelManifest, ServiceError> {
    let recommended = ModelManifest::recommended();
    if model_id == recommended.id {
        Ok(recommended)
    } else {
        Err(ServiceError::usage(format!(
            "unknown embedding model id: {model_id}"
        )))
    }
}

fn download_model_file(manifest: &ModelManifest, file_name: &str) -> io::Result<Vec<u8>> {
    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{file_name}",
        manifest.repo_id, manifest.revision
    );
    let mut response = ureq::get(&url).call().map_err(io::Error::other)?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_MODEL_FILE_BYTES)
        .read_to_vec()
        .map_err(io::Error::other)
}

fn map_embedding(error: EmbeddingError) -> ServiceError {
    match error {
        EmbeddingError::ModelNotFound(_)
        | EmbeddingError::InvalidManifest(_)
        | EmbeddingError::ChecksumMismatch { .. }
        | EmbeddingError::BatchTooLarge { .. } => ServiceError::usage(error),
        EmbeddingError::Io(_)
        | EmbeddingError::Json(_)
        | EmbeddingError::Candle(_)
        | EmbeddingError::Tokenizer(_) => ServiceError::operational(error),
    }
}
