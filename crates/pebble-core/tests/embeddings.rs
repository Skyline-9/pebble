#![forbid(unsafe_code)]

//! Consent, checksum, install, and real-inference tests for local embedding
//! models. Every fixture is synthetic and constructed in-memory; no test
//! ever performs a network request.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::embeddings::{
    ConsentState, EmbeddingError, EmbeddingRuntime, MAX_EMBED_BATCH_SIZE, ManifestFile,
    ModelManifest, ModelStore, PoolingStrategy, render_disclosure,
};
use safetensors::Dtype;
use safetensors::tensor::TensorView;
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;
use tokenizers::models::wordpiece::WordPiece;
use tokenizers::pre_tokenizers::whitespace::Whitespace;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> io::Result<Self> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-embeddings-{label}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const HIDDEN_SIZE: usize = 8;
const INTERMEDIATE_SIZE: usize = 16;
const MAX_POSITIONS: usize = 512;

const VOCAB_LINES: &[&str] = &[
    "[PAD]",
    "[UNK]",
    "[CLS]",
    "[SEP]",
    "[MASK]",
    "hello",
    "world",
    "test",
    "sentence",
    "embedding",
    "model",
    "##ing",
    "##s",
];

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest.as_slice() {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn synthetic_config() -> Vec<u8> {
    format!(
        r#"{{
            "vocab_size": {},
            "hidden_size": {HIDDEN_SIZE},
            "num_hidden_layers": 1,
            "num_attention_heads": 2,
            "intermediate_size": {INTERMEDIATE_SIZE},
            "hidden_act": "gelu",
            "hidden_dropout_prob": 0.0,
            "max_position_embeddings": {MAX_POSITIONS},
            "type_vocab_size": 2,
            "initializer_range": 0.02,
            "layer_norm_eps": 1e-12,
            "pad_token_id": 0,
            "model_type": "bert"
        }}"#,
        VOCAB_LINES.len()
    )
    .into_bytes()
}

fn synthetic_tokenizer() -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let vocab_bytes = VOCAB_LINES.join("\n").into_bytes();
    let vocab = WordPiece::read_bytes(&vocab_bytes)?;
    let model = WordPiece::builder()
        .vocab(vocab)
        .unk_token("[UNK]".to_owned())
        .build()?;
    let mut tokenizer = Tokenizer::new(model);
    tokenizer.with_pre_tokenizer(Some(Whitespace));
    Ok(tokenizer.to_string(false)?.into_bytes())
}

fn pattern_values(count: usize, seed: usize) -> Vec<f32> {
    (0..count)
        .map(|index| {
            let bucket = u16::try_from((index + seed) % 7).unwrap_or(0);
            (f32::from(bucket) - 3.0) * 0.05
        })
        .collect()
}

fn push_tensor(
    tensors: &mut Vec<(String, Vec<usize>, Vec<u8>)>,
    name: &str,
    shape: Vec<usize>,
    values: &[f32],
) {
    let mut data = Vec::with_capacity(values.len() * 4);
    for value in values {
        data.extend_from_slice(&value.to_le_bytes());
    }
    tensors.push((name.to_owned(), shape, data));
}

fn push_layer_norm(tensors: &mut Vec<(String, Vec<usize>, Vec<u8>)>, prefix: &str) {
    push_tensor(
        tensors,
        &format!("{prefix}.weight"),
        vec![HIDDEN_SIZE],
        &[1.0_f32; HIDDEN_SIZE],
    );
    push_tensor(
        tensors,
        &format!("{prefix}.bias"),
        vec![HIDDEN_SIZE],
        &[0.0_f32; HIDDEN_SIZE],
    );
}

fn push_linear(
    tensors: &mut Vec<(String, Vec<usize>, Vec<u8>)>,
    prefix: &str,
    out_dim: usize,
    in_dim: usize,
    seed: usize,
) {
    push_tensor(
        tensors,
        &format!("{prefix}.weight"),
        vec![out_dim, in_dim],
        &pattern_values(out_dim * in_dim, seed),
    );
    push_tensor(
        tensors,
        &format!("{prefix}.bias"),
        vec![out_dim],
        &vec![0.0_f32; out_dim],
    );
}

fn synthetic_weights() -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let vocab_size = VOCAB_LINES.len();
    let mut tensors: Vec<(String, Vec<usize>, Vec<u8>)> = Vec::new();

    push_tensor(
        &mut tensors,
        "embeddings.word_embeddings.weight",
        vec![vocab_size, HIDDEN_SIZE],
        &pattern_values(vocab_size * HIDDEN_SIZE, 1),
    );
    push_tensor(
        &mut tensors,
        "embeddings.position_embeddings.weight",
        vec![MAX_POSITIONS, HIDDEN_SIZE],
        &pattern_values(MAX_POSITIONS * HIDDEN_SIZE, 2),
    );
    push_tensor(
        &mut tensors,
        "embeddings.token_type_embeddings.weight",
        vec![2, HIDDEN_SIZE],
        &pattern_values(2 * HIDDEN_SIZE, 3),
    );
    push_layer_norm(&mut tensors, "embeddings.LayerNorm");

    push_linear(
        &mut tensors,
        "encoder.layer.0.attention.self.query",
        HIDDEN_SIZE,
        HIDDEN_SIZE,
        4,
    );
    push_linear(
        &mut tensors,
        "encoder.layer.0.attention.self.key",
        HIDDEN_SIZE,
        HIDDEN_SIZE,
        5,
    );
    push_linear(
        &mut tensors,
        "encoder.layer.0.attention.self.value",
        HIDDEN_SIZE,
        HIDDEN_SIZE,
        6,
    );
    push_linear(
        &mut tensors,
        "encoder.layer.0.attention.output.dense",
        HIDDEN_SIZE,
        HIDDEN_SIZE,
        7,
    );
    push_layer_norm(&mut tensors, "encoder.layer.0.attention.output.LayerNorm");
    push_linear(
        &mut tensors,
        "encoder.layer.0.intermediate.dense",
        INTERMEDIATE_SIZE,
        HIDDEN_SIZE,
        8,
    );
    push_linear(
        &mut tensors,
        "encoder.layer.0.output.dense",
        HIDDEN_SIZE,
        INTERMEDIATE_SIZE,
        9,
    );
    push_layer_norm(&mut tensors, "encoder.layer.0.output.LayerNorm");

    let views = tensors
        .iter()
        .map(|(name, shape, data)| {
            let view = TensorView::new(Dtype::F32, shape.clone(), data)?;
            Ok((name.clone(), view))
        })
        .collect::<Result<Vec<_>, safetensors::SafeTensorError>>()?;
    Ok(safetensors::serialize(views, None)?)
}

struct Fixture {
    manifest: ModelManifest,
    sources: HashMap<String, Vec<u8>>,
}

fn build_fixture(
    id: &str,
    pooling: PoolingStrategy,
) -> Result<Fixture, Box<dyn std::error::Error + Send + Sync>> {
    let config_bytes = synthetic_config();
    let weights_bytes = synthetic_weights()?;
    let tokenizer_bytes = synthetic_tokenizer()?;

    let manifest = ModelManifest {
        id: id.to_owned(),
        repo_id: "pebble-test/tiny-bert".to_owned(),
        revision: "0000000000000000000000000000000000000000".to_owned(),
        config: ManifestFile {
            name: "config.json".to_owned(),
            sha256: sha256_hex(&config_bytes),
        },
        weights: ManifestFile {
            name: "model.safetensors".to_owned(),
            sha256: sha256_hex(&weights_bytes),
        },
        tokenizer: ManifestFile {
            name: "tokenizer.json".to_owned(),
            sha256: sha256_hex(&tokenizer_bytes),
        },
        dimensions: u32::try_from(HIDDEN_SIZE)?,
        pooling,
        normalized: true,
        license: "apache-2.0".to_owned(),
        min_runtime_version: "1.0.0".to_owned(),
        approximate_install_bytes: 1,
        approximate_ram_bytes: 1,
    };

    let mut sources = HashMap::new();
    sources.insert("config.json".to_owned(), config_bytes);
    sources.insert("model.safetensors".to_owned(), weights_bytes);
    sources.insert("tokenizer.json".to_owned(), tokenizer_bytes);

    Ok(Fixture { manifest, sources })
}

fn download_from(sources: &HashMap<String, Vec<u8>>, name: &str) -> io::Result<Vec<u8>> {
    sources
        .get(name)
        .cloned()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing fixture source"))
}

fn model_dir(state_root: &Path, model_id: &str) -> PathBuf {
    state_root.join("models").join(model_id)
}

#[test]
fn consent_disclosure_is_never_skipped_without_explicit_accept()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let temp = TempDir::new("consent")?;
    let fixture = build_fixture("consent-model", PoolingStrategy::Mean)?;

    let mut consent = ConsentState::load(temp.path())?;
    assert!(!consent.warned_no_model);
    assert!(!consent.has_install_consent(&fixture.manifest.id));

    let disclosure = render_disclosure(&fixture.manifest);
    assert!(disclosure.contains(&fixture.manifest.repo_id));
    assert!(disclosure.contains(&fixture.manifest.revision));
    assert!(disclosure.contains(&fixture.manifest.weights.sha256));
    assert!(disclosure.contains(&fixture.manifest.license));

    let first_warning = consent.warn_once_if_needed();
    assert!(first_warning.is_some());
    let second_warning = consent.warn_once_if_needed();
    assert!(second_warning.is_none());

    assert!(!consent.has_install_consent(&fixture.manifest.id));
    consent.grant_install_consent(&fixture.manifest.id);
    assert!(consent.has_install_consent(&fixture.manifest.id));

    consent.save(temp.path())?;
    let reloaded = ConsentState::load(temp.path())?;
    assert_eq!(reloaded, consent);
    Ok(())
}

#[test]
fn checksum_mismatch_is_rejected_and_leaves_no_installed_model()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let temp = TempDir::new("checksum")?;
    let fixture = build_fixture("checksum-model", PoolingStrategy::Mean)?;
    let mut sources = fixture.sources;
    let mut corrupted = sources
        .get("model.safetensors")
        .cloned()
        .ok_or("missing weights source")?;
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xFF;
    sources.insert("model.safetensors".to_owned(), corrupted);

    let store = ModelStore::new(temp.path());
    let result = store.install(&fixture.manifest, |name| download_from(&sources, name));
    match result {
        Err(EmbeddingError::ChecksumMismatch { file, .. }) => {
            assert_eq!(file, "model.safetensors");
        }
        other => return Err(format!("expected a checksum mismatch, got {other:?}").into()),
    }
    assert!(store.list()?.is_empty());
    Ok(())
}

#[test]
fn install_then_load_then_embed_produces_declared_dimension_and_unit_norm()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for pooling in [PoolingStrategy::Mean, PoolingStrategy::Cls] {
        let temp = TempDir::new("embed")?;
        let fixture = build_fixture("embed-model", pooling)?;
        let store = ModelStore::new(temp.path());
        store.install(&fixture.manifest, |name| {
            download_from(&fixture.sources, name)
        })?;
        store.select(&fixture.manifest.id)?;

        let current = store.current()?.ok_or("expected a selected model")?;
        assert_eq!(current, fixture.manifest);

        let runtime = EmbeddingRuntime::load(&model_dir(temp.path(), &fixture.manifest.id))?;
        let texts = ["hello world", "test sentence embedding model"];
        let vectors = runtime.embed(&texts)?;

        assert_eq!(vectors.len(), texts.len());
        let expected_dimension = usize::try_from(fixture.manifest.dimensions)?;
        for vector in &vectors {
            assert_eq!(vector.len(), expected_dimension);
            let norm: f32 = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
            assert!(norm.is_finite());
            assert!(
                (norm - 1.0).abs() < 1e-3,
                "norm {norm} was not close to 1.0"
            );
        }
    }
    Ok(())
}

#[test]
fn oversized_text_is_bounded_by_truncation() -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let temp = TempDir::new("oversized")?;
    let fixture = build_fixture("oversized-model", PoolingStrategy::Mean)?;
    let store = ModelStore::new(temp.path());
    store.install(&fixture.manifest, |name| {
        download_from(&fixture.sources, name)
    })?;

    let runtime = EmbeddingRuntime::load(&model_dir(temp.path(), &fixture.manifest.id))?;
    let long_text = "hello world test sentence embedding model ".repeat(200);
    let vectors = runtime.embed(&[long_text.as_str()])?;

    assert_eq!(vectors.len(), 1);
    assert_eq!(
        vectors[0].len(),
        usize::try_from(fixture.manifest.dimensions)?
    );
    assert!(vectors[0].iter().all(|value| value.is_finite()));
    Ok(())
}

#[test]
fn batch_larger_than_the_limit_is_rejected() -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let temp = TempDir::new("batch")?;
    let fixture = build_fixture("batch-model", PoolingStrategy::Mean)?;
    let store = ModelStore::new(temp.path());
    store.install(&fixture.manifest, |name| {
        download_from(&fixture.sources, name)
    })?;

    let runtime = EmbeddingRuntime::load(&model_dir(temp.path(), &fixture.manifest.id))?;
    let too_many = vec!["hello"; MAX_EMBED_BATCH_SIZE + 1];
    let result = runtime.embed(&too_many);
    match result {
        Err(EmbeddingError::BatchTooLarge { actual, limit }) => {
            assert_eq!(actual, too_many.len());
            assert_eq!(limit, MAX_EMBED_BATCH_SIZE);
        }
        other => return Err(format!("expected a batch-too-large error, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn switching_models_does_not_delete_the_previous_model_until_removed()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let temp = TempDir::new("switch")?;
    let fixture_a = build_fixture("model-a", PoolingStrategy::Mean)?;
    let fixture_b = build_fixture("model-b", PoolingStrategy::Mean)?;
    let store = ModelStore::new(temp.path());

    store.install(&fixture_a.manifest, |name| {
        download_from(&fixture_a.sources, name)
    })?;
    store.select(&fixture_a.manifest.id)?;
    store.install(&fixture_b.manifest, |name| {
        download_from(&fixture_b.sources, name)
    })?;
    store.select(&fixture_b.manifest.id)?;

    let installed = store.list()?;
    assert_eq!(installed.len(), 2);
    assert!(
        model_dir(temp.path(), &fixture_a.manifest.id)
            .join("manifest.json")
            .is_file()
    );
    assert_eq!(
        store.current()?.ok_or("expected a current model")?,
        fixture_b.manifest
    );

    store.remove(&fixture_a.manifest.id)?;
    assert!(!model_dir(temp.path(), &fixture_a.manifest.id).exists());
    assert_eq!(store.list()?.len(), 1);
    assert_eq!(
        store
            .current()?
            .ok_or("expected model-b to remain current")?,
        fixture_b.manifest
    );

    store.remove(&fixture_b.manifest.id)?;
    assert!(store.current()?.is_none());
    assert!(store.list()?.is_empty());
    Ok(())
}
