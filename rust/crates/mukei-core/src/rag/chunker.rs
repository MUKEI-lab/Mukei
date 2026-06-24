//! `mukei_core::rag::chunker` — TRD §4.1.
//!
//! Splits text into 256-token windows with 32-token overlap. Operates
//! on the **whitespace-separated word stream** because the real
//! BPE-token-aware chunker lives behind the `llama-cpp` feature.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub index: u32,
    pub text: String,
    /// SHA-256 over `text` — used by usearch payload to dedup.
    pub digest: String,
}

pub struct Chunker {
    window: usize,
    overlap: usize,
}

impl Chunker {
    pub const DEFAULT_WINDOW: usize = 256;
    pub const DEFAULT_OVERLAP: usize = 32;

    pub fn new(window: usize, overlap: usize) -> Self {
        assert!(window > overlap, "window must exceed overlap");
        Self { window, overlap }
    }

    /// Split `text` into overlapping chunks.
    /// Tokens are approximated as space-separated words.
    pub fn split(&self, text: &str) -> Vec<Chunk> {
        let tokens: Vec<&str> = text.split_whitespace().collect();
        if tokens.is_empty() {
            return Vec::new();
        }

        let stride = self.window - self.overlap;
        let mut out = Vec::new();
        let mut start = 0usize;
        let mut idx = 0u32;
        while start < tokens.len() {
            let end = (start + self.window).min(tokens.len());
            let body = tokens[start..end].join(" ");
            let digest = digest(&body);
            out.push(Chunk {
                index: idx,
                text: body,
                digest,
            });
            if end == tokens.len() {
                break;
            }
            start += stride;
            idx += 1;
        }
        out
    }
}

fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    crate::diagnostics::crash_logger::hex_helper(&h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_with_overlap() {
        let c = Chunker::new(4, 1);
        let toks = (0..8)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let chunks = c.split(&toks);
        // windows: [0..4], [3..7], [6..8] -> 3 chunks
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "w0 w1 w2 w3");
        assert_eq!(chunks[1].text, "w3 w4 w5 w6");
        assert_eq!(chunks[2].text, "w6 w7");
    }

    #[test]
    fn empty_input_yields_empty() {
        let c = Chunker::default();
        assert!(c.split("").is_empty());
        assert!(c.split("   ").is_empty());
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(Self::DEFAULT_WINDOW, Self::DEFAULT_OVERLAP)
    }
}
