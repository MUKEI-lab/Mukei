//! `mukei_core::engine::tokenizer` — token counter shared with the
//! agent loop.
//!
//! # Invariants
//!
//! - Production paths use [`BpeTokenizer`] (real `tokenizers` crate).
//!   It is feature-gated behind `feature = "candle"` so the sandbox
//!   build does not depend on the tokenizer JSON.
//! - [`CharCountTokenizer`] is the test / sandbox fallback. The agent
//!   loop never picks it in shipping builds — the bridge crate
//!   constructs the right impl at boot.
//! - Both impls return at least `1` for a non-empty input so the budget
//!   manager always makes forward progress.

/// Object-safe token-count interface.
#[async_trait::async_trait]
pub trait TokenCount: Send + Sync {
    /// Count the tokens `s` would produce when fed to the active model.
    async fn count(&self, s: &str) -> usize;
}

/// Heuristic tokenizer — counts Unicode word boundaries. Used in tests
/// and on hosts where the BPE JSON is unavailable.
pub struct CharCountTokenizer;

#[async_trait::async_trait]
impl TokenCount for CharCountTokenizer {
    async fn count(&self, s: &str) -> usize {
        let mut count = 0usize;
        let mut in_word = false;
        for c in s.chars() {
            if c.is_alphanumeric() {
                if !in_word {
                    count += 1;
                    in_word = true;
                }
            } else {
                in_word = false;
            }
        }
        count.max(1)
    }
}

// ---------------------------------------------------------------------
// Real BPE tokenizer (feature = "candle")
// ---------------------------------------------------------------------

/// Wraps the `tokenizers::Tokenizer` loaded from `tokenizer.json`.
/// Construct via [`Self::from_file`] and pass to the agent loop as
/// `Arc<dyn TokenCount>`.
#[cfg(feature = "candle")]
pub struct BpeTokenizer {
    inner: tokenizers::Tokenizer,
}

#[cfg(feature = "candle")]
impl BpeTokenizer {
    /// Load a `tokenizer.json` from disk. Returns a typed
    /// [`MukeiError::ModelLoadFailed`] on failure so the bridge crate
    /// can surface a human-readable error.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> crate::error::Result<Self> {
        let path = path.as_ref();
        let inner = tokenizers::Tokenizer::from_file(path).map_err(|e| {
            crate::error::MukeiError::ModelLoadFailed(format!(
                "tokenizer load failed at {}: {e}",
                path.display()
            ))
        })?;
        Ok(Self { inner })
    }
}

#[cfg(feature = "candle")]
#[async_trait::async_trait]
impl TokenCount for BpeTokenizer {
    async fn count(&self, s: &str) -> usize {
        // The tokenizer call is cheap (single-threaded, in-process) so
        // we call it directly. If a benchmark ever shows it dominates
        // the agent loop, route it through `spawn_blocking`.
        match self.inner.encode(s, false) {
            Ok(encoded) => encoded.get_ids().len().max(1),
            Err(err) => {
                tracing::warn!(?err, "tokenizer encode failed — falling back to char count");
                CharCountTokenizer.count(s).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn char_counter_basic() {
        let t = CharCountTokenizer;
        assert_eq!(t.count("hello world").await, 2);
        assert_eq!(t.count("").await, 1);
        assert_eq!(t.count("a b c").await, 3);
    }

    #[tokio::test]
    async fn char_counter_always_at_least_one_for_empty() {
        let t = CharCountTokenizer;
        assert_eq!(t.count("").await, 1);
        assert_eq!(t.count("   ").await, 1);
    }
}
