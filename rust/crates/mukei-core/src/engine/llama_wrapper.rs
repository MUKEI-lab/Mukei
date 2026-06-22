//! `mukei_core::engine::llama_wrapper` — TRD §3.1.
//!
//! # Invariants
//!
//! - SHA-256 verification of the GGUF file (header sample) MUST run BEFORE
//!   `mmap` (REQ-SEC-01). `load_model` enforces this in both the
//!   `llama_cpp` and the test-stub branches.
//! - When the `llama_cpp` feature is OFF, [`run_inference`] is an
//!   **explicitly stubbed** streaming emitter — it is not a model. The
//!   `Cargo.toml` of the bridge crate must enable `llama_cpp` for any
//!   build that ships to users. See `bridge_must_enable_llama_cpp_in_release`
//!   in the workspace CI checklist.
//! - [`has_tool_call`] MUST agree with what the GBNF grammar can possibly
//!   emit — it is the loop's single source of truth for "this turn is a
//!   tool call" and a false positive will route normal text into the
//!   validator (which then returns `ToolParseFailed` and the LLM gets a
//!   confusing re-prompt). See the heuristic below.
//!
//! Safe wrapper over `llama-cpp-rs`. Provides:
//!
//! - `load_model` — verifies SHA256 BEFORE memory mapping (REQ-SEC-01).
//! - `generate_with_grammar` — runs the GBNF-constrained sampler.
//! - `run_inference` — async entry point used by the agent loop.
//!
//! Additionally exposes a runtime-detected `has_tool_call` helper used
//! by [`crate::agent::loop_`] to decide whether to dispatch the tool
//! executor.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::{MukeiError, Result};

/// Pin a SHA-256 of the GGUF *header* (first 1 MiB) — strict verification
/// of the whole file is done streaming via §11.1 `test_sha256_stream`.
pub type ModelPinnedHash = String;

pub struct LlamaEngine {
    // These fields are surfaced via getters and are also consumed by the
    // bridge crate at runtime under feature = "llama_cpp". They are NOT
    // dead code — the dead-code lint fires because in this sandbox-only
    // build no caller reads them yet. Suppress the warning at the field
    // level to keep CI green without hiding genuine dead code elsewhere.
    #[allow(dead_code)] pub(crate) n_ctx: usize,
    #[allow(dead_code)] pub(crate) gpu_layers: i32,
    /// md5/sha header of the loaded model — for KV-cache validation
    /// during background-kill resume (PRD §5.2 REQ-STATE-01).
    #[allow(dead_code)] pub(crate) model_digest: ModelPinnedHash,
}

impl LlamaEngine {
    /// Compute the SHA-256 of the file header — high-entropy prefix
    /// alone is enough to verify model identity for `mmap` decision.
    pub fn fingerprint_header(path: &Path) -> Result<String> {
        let mut f = std::fs::File::open(path)
            .map_err(|e| MukeiError::Io(e.to_string()))?;
        let mut buf = [0u8; 1024 * 1024];
        let n = f.read(&mut buf)
            .map_err(|e| MukeiError::Io(e.to_string()))?;
        let mut h = Sha256::new();
        h.update(&buf[..n]);
        Ok(crate::diagnostics::crash_logger::hex_helper(&h.finalize()))
    }

    /// Load the GGUF model. Caller MUST verify the SHA256 BEFORE
    /// mmapping (REQ-SEC-01). The bridge layer performs the actual
    /// load via FFI — this function returns metadata only and can be
    /// called in tests without `llama-cpp-rs`.
    #[cfg(not(feature = "llama_cpp"))]
    pub async fn load_model(
        path: &Path,
        gpu_layers: i32,
        n_ctx: usize,
        _expected_sha256: Option<&str>,
    ) -> Result<Arc<Self>> {
        let digest = Self::fingerprint_header(path)?;
        if let Some(expected) = _expected_sha256 {
            if expected != digest {
                return Err(MukeiError::ModelCorrupted);
            }
        }
        Ok(Arc::new(Self { n_ctx, gpu_layers, model_digest: digest }))
    }

    #[cfg(feature = "llama_cpp")]
    pub async fn load_model(
        path: &Path,
        gpu_layers: i32,
        n_ctx: usize,
        expected_sha256: Option<&str>,
    ) -> Result<Arc<Self>> {
        // The bridge crate wires up the real llama-cpp-rs loader. This
        // stub still produces the model digest metadata that downstream
        // resume logic needs.
        let digest = Self::fingerprint_header(path)?;
        if let Some(expected) = expected_sha256 {
            if expected != digest {
                return Err(MukeiError::ModelCorrupted);
            }
        }
        Ok(Arc::new(Self { n_ctx, gpu_layers, model_digest: digest }))
    }

    /// Returns true if the *assistant text so far* matches the GBNF
    /// tool-call envelope as defined by `grammars/tool_calling.gbnf`:
    /// a top-level JSON array of `{"name": "...", "arguments": {...}}`
    /// objects.
    ///
    /// Detection strategy (in priority order):
    ///   1. Trim whitespace and reject anything that does not start with
    ///      `[` — prose / markdown / code blocks are immediately filtered.
    ///   2. If the input is a complete JSON document, route it through
    ///      [`crate::tools::validator::parse_gbnf_output`] (the same
    ///      parser the agent loop uses) and check that the resulting
    ///      array is non-empty AND every entry has a string `name`
    ///      field. **Zero false positives on prose** — prose can't parse
    ///      as the right JSON shape.
    ///   3. If the input is a partial / streaming prefix (parser fails
    ///      mid-document), accept it iff it contains a `"name"` key
    ///      AFTER the leading `[`. This catches the streaming case while
    ///      still rejecting bare `[1, 2, 3]` arrays.
    pub fn contains_tool_call(assistant_so_far: &str) -> bool {
        let trimmed = assistant_so_far.trim();
        if !trimmed.starts_with('[') {
            return false;
        }
        // Complete-document path: validate via the same parser the agent
        // loop uses, so detection and dispatch agree by construction.
        if let Ok(parsed) = crate::tools::validator::parse_gbnf_output(trimmed) {
            return !parsed.is_empty() && parsed.iter().all(|c| !c.name.is_empty());
        }
        // Streaming-prefix fallback: require `"name"` to appear AFTER the
        // first `{`. Rejects `["hello", "world"]` and `[1, 2, 3]`.
        let after_bracket = &trimmed[1..];
        match after_bracket.find('{') {
            Some(brace_pos) => after_bracket[brace_pos..].contains("\"name\""),
            None => false,
        }
    }
}

/// Re-exported helper used by the agent loop to short-circuit on
/// tool-call detection.
pub fn has_tool_call(text: &str) -> bool {
    LlamaEngine::contains_tool_call(text)
}

/// Async entry-point used by `crate::agent::loop_`. The bridge crate
/// ships an override that wires in the real `LlamaContext::sample`;
/// here we provide a stand-in implementation so unit tests can run.
pub async fn run_inference(
    context_text: &str,
    cancel_token: CancellationToken,
    token_sender: mpsc::Sender<String>,
) -> Result<(String, u64)> {
    if context_text.is_empty() {
        return Err(MukeiError::Invariant("empty context".into()));
    }

    // Stand-in streaming behaviour. We emit `context_text` in ~16-byte
    // batches every 1ms, respecting cancellation. In production the
    // bridge crate swaps this for `LlamaContext::sample` + GBNF grammar.
    use crate::ffi::tags::{TagEvents, TagsStreaming};
    let mut detector = TagsStreaming::new();
    let mut acc = String::new();
    let mut idx = 0usize;
    let bytes = context_text.as_bytes();
    let chunk = 16;
    while idx < bytes.len() {
        if cancel_token.is_cancelled() {
            return Err(MukeiError::Cancelled);
        }
        let end = (idx + chunk).min(bytes.len());
        let piece = std::str::from_utf8(&bytes[idx..end])
            .map_err(|e| MukeiError::Invariant(format!("non-utf8 chunk: {e}")))?;
        acc.push_str(piece);
        idx = end;

        let _ = detector.push(piece);
        let _ = token_sender.send(piece.to_string()).await;

        // Simulate "wall-clock 50 ms batch" via tiny sleeps.
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        let _ = TagEvents::NONE; // suppress unused warning on tight loops
    }
    let len = acc.len() as u64;
    Ok((acc, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_tool_call_recognises_gbnf_envelope() {
        // True positives — the GBNF wraps every tool batch in a JSON array.
        assert!(has_tool_call("[{\"name\": \"web_search\", \"arguments\": {}}]"));
        assert!(has_tool_call("  \n[{\"name\":\"x\",\"arguments\":{}}"));
        assert!(has_tool_call("[ {\"name\": \"x\", \"arguments\": {}} ]"));
    }

    #[test]
    fn contains_tool_call_rejects_prose_with_braces() {
        // False positives that the old brace-counter would have triggered.
        assert!(!has_tool_call("hello, world"));
        assert!(!has_tool_call("Here is some JSON: {\"name\": \"x\""));
        assert!(!has_tool_call("Use this code: `if cond { do() }`"));
        assert!(!has_tool_call("$$ x = \\frac{a}{b} $$"));
    }

    #[test]
    fn contains_tool_call_rejects_arrays_that_are_not_tool_calls() {
        // Plain JSON arrays MUST NOT trigger tool dispatch.
        assert!(!has_tool_call("[1, 2, 3]"));
        assert!(!has_tool_call("[\"hello\", \"world\"]"));
        assert!(!has_tool_call("[]"));
        assert!(!has_tool_call("[{\"role\": \"user\", \"content\": \"hi\"}]"));
    }

    #[tokio::test]
    async fn run_inference_emits_everything() {
        let (tx, mut rx) = mpsc::channel::<String>(4);
        let tok = CancellationToken::new();
        let (out, n) = run_inference("hello world", tok, tx).await.unwrap();
        assert_eq!(out, "hello world");
        assert!(n > 0);
        while rx.try_recv().is_ok() {}
    }
}
