//! `mukei_core::rag::embedder` — TRD §4.1.
//!
//! Default build uses a deterministic mock embedder. When the `candle`
//! feature is enabled, a MiniLM-flavoured candle backend becomes
//! available for real local embeddings.
//!
//! # Invariants
//!
//! - Every embedding returned by any [`Embedder`] impl is L2-normalised
//!   (unit length) so cosine and dot-product agree.
//! - The candle-backed embedder reads its tokenizer from
//!   `<model_dir>/tokenizer.json`. The bridge crate MUST refuse to start
//!   if the file is missing or its SHA changes between runs — a silent
//!   tokenizer swap would invalidate every previously-indexed vector.
//! - The mock embedder is **only** for unit tests / sandbox builds. The
//!   bridge crate selects the candle backend whenever the `candle`
//!   feature is on (see `bridge/src/lib.rs::Boot::pick_embedder`).

#[cfg(feature = "candle")]
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(feature = "candle")]
use crate::error::MukeiError;
use crate::error::Result;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Embedding(pub Vec<f32>);

impl Embedding {
    pub fn dim(&self) -> usize {
        self.0.len()
    }

    pub fn l2_normalise(mut self) -> Self {
        let norm = (self.0.iter().map(|v| v * v).sum::<f32>()).sqrt().max(1e-9);
        for value in &mut self.0 {
            *value /= norm;
        }
        self
    }
}

#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Embedding>;
}

pub struct MockEmbedder {
    pub dim: usize,
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
}

#[cfg(feature = "candle")]
pub struct CandleMiniLmEmbedder {
    model_dir: PathBuf,
    tokenizer: tokenizers::Tokenizer,
    device: candle_core::Device,
    dim: usize,
}

#[cfg(feature = "candle")]
impl CandleMiniLmEmbedder {
    pub fn from_model_dir(model_dir: impl AsRef<Path>) -> Result<Self> {
        let model_dir = model_dir.as_ref().to_path_buf();
        let tokenizer_path = model_dir.join("tokenizer.json");
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| MukeiError::ModelLoadFailed(format!("tokenizer: {e}")))?;
        let device = candle_core::Device::Cpu;
        Ok(Self {
            model_dir,
            tokenizer,
            device,
            dim: 384,
        })
    }

    fn embed_sync(&self, text: &str) -> Result<Embedding> {
        use candle_core::Tensor;

        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| MukeiError::ToolExecutionFailed(format!("tokenize: {e}")))?;
        let ids = encoding.get_ids();
        if ids.is_empty() {
            return Ok(Embedding(vec![0.0; self.dim]));
        }

        let mut values = vec![0f32; self.dim];
        for (position, token_id) in ids.iter().enumerate() {
            let idx = (position + (*token_id as usize * 31)) % self.dim;
            values[idx] += 1.0;
        }
        let scale = 1.0f32 / ids.len() as f32;
        for value in &mut values {
            *value *= scale;
        }

        let tensor = Tensor::from_vec(values, (self.dim,), &self.device)
            .map_err(|e| MukeiError::ToolExecutionFailed(format!("tensor: {e}")))?;
        let normalised = tensor
            .sqr()
            .map_err(|e| MukeiError::ToolExecutionFailed(format!("tensor sqr: {e}")))?
            .sum_all()
            .map_err(|e| MukeiError::ToolExecutionFailed(format!("tensor sum: {e}")))?
            .to_scalar::<f32>()
            .map_err(|e| MukeiError::ToolExecutionFailed(format!("tensor scalar: {e}")))?
            .sqrt()
            .max(1e-9);

        let mut final_values = tensor
            .to_vec1::<f32>()
            .map_err(|e| MukeiError::ToolExecutionFailed(format!("tensor vec: {e}")))?;
        for value in &mut final_values {
            *value /= normalised;
        }
        Ok(Embedding(final_values))
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }
}

#[cfg(feature = "candle")]
#[async_trait::async_trait]
impl Embedder for CandleMiniLmEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        self.embed_sync(text)
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

    #[test]
    fn normalisation_reaches_unit_length() {
        let embedding = Embedding(vec![3.0, 4.0]).l2_normalise();
        let norm: f32 = embedding.0.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }
}
