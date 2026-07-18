//! Real local BERT-family inference for consent-gated embedding models.
//!
//! Loads pinned safetensors weights and a tokenizer from an installed
//! model's directory (see [`super::model::ModelStore`]) and runs a genuine
//! CPU forward pass to produce pooled, optionally L2-normalized embedding
//! vectors.

use std::fs;
use std::path::Path;

use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig, DTYPE};
use tokenizers::{
    Encoding, PaddingDirection, PaddingParams, PaddingStrategy, Tokenizer, TruncationDirection,
    TruncationParams, TruncationStrategy,
};

use super::EmbeddingError;
use super::manifest::{ModelManifest, PoolingStrategy};

/// Maximum accepted token length per text; longer inputs are truncated.
pub const MAX_EMBED_TEXT_TOKENS: usize = 512;
/// Maximum accepted number of texts in a single [`EmbeddingRuntime::embed`] call.
pub const MAX_EMBED_BATCH_SIZE: usize = 64;

/// Loaded local BERT-family embedding model ready for CPU inference.
pub struct EmbeddingRuntime {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    pooling: PoolingStrategy,
    normalized: bool,
}

impl EmbeddingRuntime {
    /// Load model weights, configuration, and tokenizer from `model_dir` (as
    /// laid out by [`super::model::ModelStore`]).
    ///
    /// # Errors
    ///
    /// Returns an error when the manifest, weights, configuration, or
    /// tokenizer cannot be read or fail to parse.
    pub fn load(model_dir: &Path) -> Result<Self, EmbeddingError> {
        let manifest_bytes = fs::read(model_dir.join("manifest.json"))?;
        let manifest: ModelManifest = serde_json::from_slice(&manifest_bytes)?;
        manifest.validate()?;
        let config_bytes = fs::read(model_dir.join(&manifest.config.name))?;
        let config: BertConfig = serde_json::from_slice(&config_bytes)?;
        let weights = fs::read(model_dir.join(&manifest.weights.name))?;
        let tokenizer_bytes = fs::read(model_dir.join(&manifest.tokenizer.name))?;

        let device = Device::Cpu;
        let var_builder = VarBuilder::from_buffered_safetensors(weights, DTYPE, &device)?;
        let model = BertModel::load(var_builder, &config)?;
        let tokenizer = load_tokenizer(&tokenizer_bytes)?;

        Ok(Self {
            model,
            tokenizer,
            device,
            pooling: manifest.pooling,
            normalized: manifest.normalized,
        })
    }

    /// Embed `texts` into mean- or CLS-pooled, optionally L2-normalized
    /// vectors matching the loaded manifest's declared dimension.
    ///
    /// Texts longer than [`MAX_EMBED_TEXT_TOKENS`] tokens are truncated; an
    /// empty batch returns an empty result without running inference.
    ///
    /// # Errors
    ///
    /// Returns an error when the batch exceeds [`MAX_EMBED_BATCH_SIZE`],
    /// tokenization fails, or the forward pass fails.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if texts.len() > MAX_EMBED_BATCH_SIZE {
            return Err(EmbeddingError::BatchTooLarge {
                actual: texts.len(),
                limit: MAX_EMBED_BATCH_SIZE,
            });
        }
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(EmbeddingError::Tokenizer)?;
        let (input_ids, token_type_ids, attention_mask) =
            stack_encodings(&encodings, &self.device)?;
        let hidden = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?;
        let pooled = pool(&hidden, &attention_mask, self.pooling)?;
        let output = if self.normalized {
            l2_normalize(&pooled)?
        } else {
            pooled
        };
        Ok(output.to_vec2::<f32>()?)
    }
}

fn load_tokenizer(bytes: &[u8]) -> Result<Tokenizer, EmbeddingError> {
    let mut tokenizer = Tokenizer::from_bytes(bytes).map_err(EmbeddingError::Tokenizer)?;
    tokenizer
        .with_truncation(Some(TruncationParams {
            direction: TruncationDirection::Right,
            max_length: MAX_EMBED_TEXT_TOKENS,
            strategy: TruncationStrategy::LongestFirst,
            stride: 0,
        }))
        .map_err(EmbeddingError::Tokenizer)?;
    tokenizer.with_padding(Some(PaddingParams {
        strategy: PaddingStrategy::BatchLongest,
        direction: PaddingDirection::Right,
        pad_to_multiple_of: None,
        pad_id: 0,
        pad_type_id: 0,
        pad_token: "[PAD]".to_owned(),
    }));
    Ok(tokenizer)
}

#[allow(clippy::cast_precision_loss)] // attention-mask values are always 0 or 1
fn stack_encodings(
    encodings: &[Encoding],
    device: &Device,
) -> Result<(Tensor, Tensor, Tensor), EmbeddingError> {
    let batch = encodings.len();
    let width = encodings
        .first()
        .map_or(0, |encoding| encoding.get_ids().len());
    let mut ids = Vec::with_capacity(batch * width);
    let mut types = Vec::with_capacity(batch * width);
    let mut mask = Vec::with_capacity(batch * width);
    for encoding in encodings {
        ids.extend_from_slice(encoding.get_ids());
        types.extend_from_slice(encoding.get_type_ids());
        mask.extend(
            encoding
                .get_attention_mask()
                .iter()
                .map(|&value| value as f32),
        );
    }
    let input_ids = Tensor::from_vec(ids, (batch, width), device)?;
    let token_type_ids = Tensor::from_vec(types, (batch, width), device)?;
    let attention_mask = Tensor::from_vec(mask, (batch, width), device)?;
    Ok((input_ids, token_type_ids, attention_mask))
}

fn pool(
    hidden: &Tensor,
    attention_mask: &Tensor,
    strategy: PoolingStrategy,
) -> Result<Tensor, EmbeddingError> {
    match strategy {
        PoolingStrategy::Cls => Ok(hidden.narrow(1, 0, 1)?.squeeze(1)?),
        PoolingStrategy::Mean => mean_pool(hidden, attention_mask),
    }
}

fn mean_pool(hidden: &Tensor, attention_mask: &Tensor) -> Result<Tensor, EmbeddingError> {
    let mask = attention_mask.unsqueeze(2)?;
    let masked = hidden.broadcast_mul(&mask)?;
    let summed = masked.sum(1)?;
    let counts = attention_mask
        .sum(1)?
        .clamp(1e-9_f32, f32::MAX)?
        .unsqueeze(1)?;
    Ok(summed.broadcast_div(&counts)?)
}

fn l2_normalize(vectors: &Tensor) -> Result<Tensor, EmbeddingError> {
    let norm = vectors
        .sqr()?
        .sum_keepdim(1)?
        .sqrt()?
        .clamp(1e-12_f32, f32::MAX)?;
    Ok(vectors.broadcast_div(&norm)?)
}
