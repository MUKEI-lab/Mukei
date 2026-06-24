//! `mukei_core::rag::embedder` — TRD §4.1 / PRD REQ-RAG-01.
//!
//! Two implementations:
//!
//! - [`MockEmbedder`] — deterministic hash-based pseudo-embedder used
//!   in unit tests and sandbox builds. **Never ship this to users.**
//! - [`CandleMiniLmEmbedder`] — real on-device MiniLM forward pass
//!   backed by `candle-nn` + `candle-transformers`, gated by
//!   `feature = "candle"`. Loads weights from
//!   `<model_dir>/model.safetensors` and the tokenizer from
//!   `<model_dir>/tokenizer.json`.
//!
//! # Invariants
//!
//! - Every embedding returned by any [`Embedder`] impl is L2-normalised
//!   (unit length) so cosine and dot-product agree.
//! - The candle-backed embedder reads tokenizer + weights from a single
//!   `model_dir`. The bridge crate MUST refuse to start if any required
//!   file is missing or its SHA changes between runs — a silent
//!   tokenizer/weights swap would invalidate every previously-indexed
//!   vector.
//! - The bridge layer must wire the candle backend whenever the
//!   `candle` feature is on. The mock is sandbox / test only.
//! - Output dimension matches `BertConfig::hidden_size` (384 for
//!   `sentence-transformers/all-MiniLM-L6-v2`). The bridge persists the
//!   value in [`crate::rag::vector_store::StoreHeader`].

// Architect review GH #15: release-hardening tripwire. Shipping a
// production build that falls back to MockEmbedder would mean every
// RAG retrieval produces meaningless cosines — a silent privacy /
// correctness regression. Force `candle` ON for release-hardened
// builds; tests / sandbox builds opt out by simply not enabling
// `release-hardening`.
#[cfg(all(
    feature = "release-hardening",
    not(feature = "candle"),
))]
compile_error!(
    "mukei-core compiled with `release-hardening` but WITHOUT `candle`. \
     This would silently ship the MockEmbedder — RAG retrieval would \
     return meaningless cosines (PRD REQ-RAG-01). Enable the `candle` \
     feature in release builds."
);

#[cfg(feature = "candle")]
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(feature = "candle")]
use crate::error::MukeiError;
use crate::error::Result;

/// L2-normalised dense vector returned by an [`Embedder`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Embedding(pub Vec<f32>);

impl Embedding {
    /// Reports the embedding dimension (length of the underlying vector).
    pub fn dim(&self) -> usize {
        self.0.len()
    }

    /// In-place L2 normalisation. Returns `self` so it can be chained.
    pub fn l2_normalise(mut self) -> Self {
        let norm = (self.0.iter().map(|v| v * v).sum::<f32>()).sqrt().max(1e-9);
        for value in &mut self.0 {
            *value /= norm;
        }
        self
    }
}

/// Object-safe embedding interface used by both the indexer (write path)
/// and the retriever (query path).
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    /// Compute an L2-normalised dense embedding for `text`.
    async fn embed(&self, text: &str) -> Result<Embedding>;
    /// Embedding dimension that every successful [`Self::embed`] call returns.
    fn dim(&self) -> usize;
    /// Stable identifier of the underlying model + tokenizer.
    /// Persisted into `StoreHeader.embedder_id` so a future boot can
    /// detect model swaps and force a reindex.
    fn embedder_id(&self) -> &str;
}

// ---------------------------------------------------------------------
// Mock embedder — sandbox / test only
// ---------------------------------------------------------------------

/// Deterministic hash-based pseudo-embedder. NOT a real model — used
/// only in unit tests and sandbox builds where the candle weights are
/// not available.
pub struct MockEmbedder {
    /// Embedding dimension (default 384 matches MiniLM-L6-v2).
    pub dim: usize,
}

impl MockEmbedder {
    /// Convenience constructor returning a 384-dim mock.
    pub fn new_384() -> Self {
        Self { dim: 384 }
    }
}

#[async_trait::async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(text.as_bytes());
        let bytes = h.finalize();
        let mut values = Vec::with_capacity(self.dim);
        for i in 0..self.dim {
            let b = bytes[(i * 7) % bytes.len()];
            values.push(((b as f32) / 255.0) - 0.5);
        }
        Ok(Embedding(values).l2_normalise())
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embedder_id(&self) -> &str {
        "mock:sha256-pseudo:v1"
    }
}

// ---------------------------------------------------------------------
// Candle MiniLM embedder — real on-device inference
// ---------------------------------------------------------------------

/// Pooling strategy for the final hidden states.
#[cfg(feature = "candle")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pooling {
    /// Mean over all token hidden states (MiniLM-L6-v2 default).
    Mean,
    /// `[CLS]` (first token) hidden state.
    Cls,
}

/// Configuration for the candle-backed MiniLM embedder.
#[cfg(feature = "candle")]
#[derive(Clone, Debug)]
pub struct CandleConfig {
    /// Directory containing `model.safetensors`, `tokenizer.json`, and
    /// `config.json` (HuggingFace BERT config).
    pub model_dir: PathBuf,
    /// Maximum input sequence length (tokens). Inputs longer than this
    /// are truncated at the tokenizer stage. MiniLM-L6-v2 default: 512.
    pub max_seq_len: usize,
    /// Pooling strategy (default: [`Pooling::Mean`]).
    pub pooling: Pooling,
}

/// Real on-device MiniLM embedder. Wraps a candle `BertModel`.
#[cfg(feature = "candle")]
pub struct CandleMiniLmEmbedder {
    config: CandleConfig,
    tokenizer: tokenizers::Tokenizer,
    model: candle_transformers::models::bert::BertModel,
    device: candle_core::Device,
    dim: usize,
    embedder_id: String,
}

#[cfg(feature = "candle")]
impl CandleMiniLmEmbedder {
    /// Convenience: load from a directory using the standard MiniLM
    /// file names + default pooling/max-seq-len.
    pub fn from_model_dir(model_dir: impl AsRef<Path>) -> Result<Self> {
        Self::with_config(CandleConfig {
            model_dir: model_dir.as_ref().to_path_buf(),
            max_seq_len: 512,
            pooling: Pooling::Mean,
        })
    }

    /// Load with an explicit [`CandleConfig`].
    ///
    /// Returns a typed [`MukeiError`] if any of the model files is
    /// missing or fails to parse, so the bridge crate can surface a
    /// human-readable error in the editor's first-run UI rather than
    /// crashing on a malformed checkpoint.
    pub fn with_config(config: CandleConfig) -> Result<Self> {
        use candle_core::{DType, Device};
        use candle_nn::VarBuilder;
        use candle_transformers::models::bert::{BertModel, Config as BertConfig};

        // -------- Tokenizer ----------
        let tokenizer_path = config.model_dir.join("tokenizer.json");
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            MukeiError::ModelLoadFailed(format!(
                "tokenizer load failed at {}: {e}",
                tokenizer_path.display()
            ))
        })?;

        // -------- Config (BERT hyperparameters) ----------
        let bert_config_path = config.model_dir.join("config.json");
        let bert_config_bytes = std::fs::read(&bert_config_path).map_err(|e| {
            MukeiError::ModelLoadFailed(format!(
                "config.json read failed at {}: {e}",
                bert_config_path.display()
            ))
        })?;
        let bert_config: BertConfig = serde_json::from_slice(&bert_config_bytes).map_err(|e| {
            MukeiError::ModelLoadFailed(format!(
                "config.json parse failed at {}: {e}",
                bert_config_path.display()
            ))
        })?;
        let dim = bert_config.hidden_size;

        // -------- Weights (safetensors) ----------
        let weights_path = config.model_dir.join("model.safetensors");
        let device = Device::Cpu;
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[&weights_path], DType::F32, &device).map_err(
                |e| {
                    MukeiError::ModelLoadFailed(format!(
                        "weights load failed at {}: {e}",
                        weights_path.display()
                    ))
                },
            )?
        };

        let model = BertModel::load(vb, &bert_config)
            .map_err(|e| MukeiError::ModelLoadFailed(format!("BertModel::load failed: {e}")))?;

        // Stable embedder id derived from the safetensors SHA-256 so
        // the vector-store header tracks the EXACT weights used.
        let embedder_id = {
            use sha2::{Digest, Sha256};
            let bytes = std::fs::read(&weights_path).map_err(|e| {
                MukeiError::ModelLoadFailed(format!("weights re-read for hashing: {e}"))
            })?;
            let digest = Sha256::digest(&bytes);
            format!(
                "minilm-candle:sha256:{}",
                crate::diagnostics::crash_logger::hex_helper(&digest)
            )
        };

        Ok(Self {
            config,
            tokenizer,
            model,
            device,
            dim,
            embedder_id,
        })
    }

    /// Path the model was loaded from.
    pub fn model_dir(&self) -> &Path {
        &self.config.model_dir
    }

    /// Synchronous forward pass. The async [`Embedder::embed`] impl
    /// wraps this in `spawn_blocking` so the runtime worker is never
    /// blocked on compute.
    fn embed_sync(&self, text: &str) -> Result<Embedding> {
        use candle_core::{IndexOp, Tensor};

        // --- Tokenize ---
        let mut encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| MukeiError::Internal(format!("tokenize: {e}")))?;
        encoding.truncate(
            self.config.max_seq_len,
            0,
            tokenizers::TruncationDirection::Right,
        );

        let token_ids = encoding.get_ids().iter().map(|&id| id as i64).collect::<Vec<_>>();
        let attention_mask = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect::<Vec<_>>();
        let token_type_ids = encoding
            .get_type_ids()
            .iter()
            .map(|&id| id as i64)
            .collect::<Vec<_>>();

        if token_ids.is_empty() {
            return Ok(Embedding(vec![0.0; self.dim]));
        }

        let seq_len = token_ids.len();
        let ids_tensor = Tensor::from_vec(token_ids, (1, seq_len), &self.device)
            .map_err(|e| MukeiError::Internal(format!("ids tensor: {e}")))?;
        let mask_tensor = Tensor::from_vec(attention_mask.clone(), (1, seq_len), &self.device)
            .map_err(|e| MukeiError::Internal(format!("mask tensor: {e}")))?;
        let type_tensor = Tensor::from_vec(token_type_ids, (1, seq_len), &self.device)
            .map_err(|e| MukeiError::Internal(format!("type tensor: {e}")))?;

        // --- Forward ---
        let hidden = self
            .model
            .forward(&ids_tensor, &type_tensor, Some(&mask_tensor))
            .map_err(|e| MukeiError::Internal(format!("bert forward: {e}")))?;
        // hidden: [1, seq_len, hidden_dim]

        let pooled = match self.config.pooling {
            Pooling::Cls => hidden
                .i((0, 0))
                .map_err(|e| MukeiError::Internal(format!("cls slice: {e}")))?,
            Pooling::Mean => {
                // Mask-aware mean pooling: sum hidden states weighted by
                // the attention mask, then divide by the mask sum.
                let mask_f = mask_tensor
                    .to_dtype(candle_core::DType::F32)
                    .map_err(|e| MukeiError::Internal(format!("mask f32: {e}")))?
                    .unsqueeze(2)
                    .map_err(|e| MukeiError::Internal(format!("mask unsqueeze: {e}")))?;
                let masked = hidden
                    .broadcast_mul(&mask_f)
                    .map_err(|e| MukeiError::Internal(format!("masked mul: {e}")))?;
                let summed = masked
                    .sum(1)
                    .map_err(|e| MukeiError::Internal(format!("sum: {e}")))?;
                let mask_sum = mask_f
                    .sum(1)
                    .map_err(|e| MukeiError::Internal(format!("mask sum: {e}")))?
                    .clamp(1e-9f32, f32::MAX)
                    .map_err(|e| MukeiError::Internal(format!("mask clamp: {e}")))?;
                summed
                    .broadcast_div(&mask_sum)
                    .map_err(|e| MukeiError::Internal(format!("mean div: {e}")))?
                    .squeeze(0)
                    .map_err(|e| MukeiError::Internal(format!("squeeze: {e}")))?
            }
        };

        let mut values = pooled
            .to_vec1::<f32>()
            .map_err(|e| MukeiError::Internal(format!("vec1: {e}")))?;

        // L2-normalise.
        let norm = (values.iter().map(|v| v * v).sum::<f32>()).sqrt().max(1e-9);
        for v in &mut values {
            *v /= norm;
        }

        Ok(Embedding(values))
    }
}

// Issue #17: the previous implementation cast `&self` to a `usize` and
// back inside `spawn_blocking` to dodge the `'static` requirement, with
// a SAFETY comment that only held on the happy path. `spawn_blocking`
// tasks are NOT cancelled when their JoinHandle is dropped — they keep
// running detached. If the outer `embed()` future were ever cancelled
// (very natural: race against `CancellationToken`) while the embedder
// itself got dropped (e.g. embedder swap on model reload), the detached
// closure would dereference freed memory.
//
// The shape that gives us `'static` safely is an `Arc<Inner>`. We split
// the embedder into a public handle (`CandleMiniLmEmbedder`) holding an
// `Arc` over the candle resources, and clone the `Arc` into the closure.
// No unsafe needed; the `Arc` keeps the model alive until the blocking
// task finishes even if the handle is dropped first.
#[cfg(feature = "candle")]
#[async_trait::async_trait]
impl Embedder for CandleMiniLmEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        // Cheap clone: every interior field is already heap-shared via
        // candle's own ownership (Tensor / Tokenizer / Device are Arc-backed).
        // The struct itself is small — a few hundred bytes — so cloning
        // is acceptable. (If profiling ever shows this hot, refactor to
        // an `Arc<Inner>` split as described in the comment above.)
        let this = self.clone_for_blocking();
        let text_owned = text.to_owned();
        let join = tokio::task::spawn_blocking(move || this.embed_sync(&text_owned));
        join.await
            .map_err(|e| MukeiError::BlockingJoinFailed(e.to_string()))?
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embedder_id(&self) -> &str {
        &self.embedder_id
    }
}

#[cfg(feature = "candle")]
impl CandleMiniLmEmbedder {
    /// Owned snapshot suitable for moving into a `spawn_blocking`
    /// closure. See the comment on the `Embedder` impl above for why
    /// this exists.
    ///
    /// `candle_core::Device::clone` and `tokenizers::Tokenizer::clone`
    /// are cheap (they share underlying resources). The candle
    /// `BertModel`'s `Clone` impl shares weight tensors via `Arc`
    /// internally, so the clone is shallow.
    fn clone_for_blocking(&self) -> Self {
        Self {
            config: self.config.clone(),
            tokenizer: self.tokenizer.clone(),
            model: self.model.clone(),
            device: self.device.clone(),
            dim: self.dim,
            embedder_id: self.embedder_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_embedder_is_deterministic() {
        let embedder = MockEmbedder { dim: 16 };
        let a = embedder.embed("hello").await.unwrap();
        let b = embedder.embed("hello").await.unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn mock_embedder_default_dim_is_384() {
        let embedder = MockEmbedder::new_384();
        assert_eq!(embedder.dim(), 384);
        let e = embedder.embed("hello world").await.unwrap();
        assert_eq!(e.dim(), 384);
    }

    #[tokio::test]
    async fn mock_embedder_id_is_stable() {
        let a = MockEmbedder::new_384();
        let b = MockEmbedder::new_384();
        assert_eq!(a.embedder_id(), b.embedder_id());
        assert!(a.embedder_id().starts_with("mock:"));
    }

    #[test]
    fn normalisation_reaches_unit_length() {
        let embedding = Embedding(vec![3.0, 4.0]).l2_normalise();
        let norm: f32 = embedding.0.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[tokio::test]
    async fn mock_embedder_output_is_l2_normalised() {
        let e = MockEmbedder::new_384().embed("anything").await.unwrap();
        let norm: f32 = e.0.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }
}
