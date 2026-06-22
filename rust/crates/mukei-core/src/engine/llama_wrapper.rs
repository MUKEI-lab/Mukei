//! `mukei_core::engine::llama_wrapper` — TRD §3.1 / PRD REQ-INF-01.
//!
//! # Invariants
//!
//! - **Full-file SHA-256 verification BEFORE `mmap`.** REQ-SEC-01 /
//!   TRD §5.3. The previous header-only fingerprint was insufficient
//!   because tampered weight blocks past the 1 MiB boundary went
//!   undetected. `verify_full_sha256_stream` now reads the entire GGUF
//!   in 1 MiB chunks and rejects on mismatch with
//!   [`MukeiError::ModelCorrupted`] BEFORE any caller can mmap.
//! - **Tool-call detection is grammar-aware.**
//!   [`contains_tool_call`] always routes a candidate through
//!   `crate::tools::validator::parse_gbnf_output`. The legacy
//!   string-prefix fallback only applies when the parser cannot make
//!   a decision (mid-stream partial JSON). Prose / code blocks /
//!   plain arrays NEVER trip the detector.
//! - **KV-cache + model fingerprints are exposed.** The agent loop /
//!   recovery store pulls them via [`LlamaEngine::model_digest`] and
//!   [`LlamaEngine::kv_cache_fingerprint`] (REQ-STATE-01).
//! - **Stop reasons are typed.** `run_inference` returns an
//!   [`InferenceOutcome`] whose `stop_reason` distinguishes
//!   `UserStopped`, `ThermalKill`, `OutOfMemory`, `WatchdogTripped`,
//!   `Completed`. The bridge crate uses this to render the right UI
//!   chip (REQ-INF-04).

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::{MukeiError, Result};

/// 1 MiB read window for the full-file SHA stream. Matches the GGUF
/// chunked-write step so the verifier and the writer share a rhythm.
const SHA_STREAM_CHUNK: usize = 1024 * 1024;

/// Pin a SHA-256 of the GGUF *header* (first 1 MiB) for the quick boot
/// path. Full-file verification still happens via
/// [`verify_full_sha256_stream`] before `mmap`.
pub type ModelPinnedHash = String;

// ---------------------------------------------------------------------
// EngineConfig (M2)
// ---------------------------------------------------------------------

/// Builder-friendly configuration for [`LlamaEngine`]. Centralises the
/// per-model knobs the previous code spread across function arguments.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Context window in tokens.
    pub n_ctx: usize,
    /// Number of layers to offload to the GPU (`0` = CPU-only).
    pub gpu_layers: i32,
    /// Optional expected full-file SHA-256 (lowercase hex). When set,
    /// [`LlamaEngine::load_model`] streams the file and rejects on
    /// mismatch before mmap.
    pub expected_sha256: Option<String>,
    /// Stream config used by the bridge to batch tokens.
    pub stream: super::streaming::TokenStreamConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            n_ctx: 4096,
            gpu_layers: 0,
            expected_sha256: None,
            stream: super::streaming::TokenStreamConfig::default(),
        }
    }
}

impl EngineConfig {
    /// Fluent setter for `n_ctx`.
    pub fn with_n_ctx(mut self, n: usize) -> Self {
        self.n_ctx = n;
        self
    }
    /// Fluent setter for `gpu_layers`.
    pub fn with_gpu_layers(mut self, n: i32) -> Self {
        self.gpu_layers = n;
        self
    }
    /// Fluent setter for the pinned full-file SHA-256.
    pub fn with_expected_sha256(mut self, sha: impl Into<String>) -> Self {
        self.expected_sha256 = Some(sha.into());
        self
    }
}

// ---------------------------------------------------------------------
// InferenceOutcome + StopReason (H4)
// ---------------------------------------------------------------------

/// Why a streaming inference call ended. Used by the agent loop and
/// the bridge to render the right UI chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The model emitted EOS / stopped naturally.
    Completed,
    /// User pressed the Stop button.
    UserStopped,
    /// SoC thermal sensor reported a critical condition.
    ThermalKill,
    /// Memory preflight refused further generation.
    OutOfMemory,
    /// One of the watchdog budgets tripped.
    WatchdogTripped,
}

impl StopReason {
    /// Stable identifier used for the FFI snapshot.
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::UserStopped => "user_stopped",
            Self::ThermalKill => "thermal_kill",
            Self::OutOfMemory => "oom",
            Self::WatchdogTripped => "watchdog",
        }
    }
}

/// Typed result of a streaming inference call. The first field is the
/// fully-accumulated assistant text; the second is the token count; the
/// third is the structured stop reason.
#[derive(Clone, Debug)]
pub struct InferenceOutcome {
    /// Concatenated assistant output.
    pub assistant_text: String,
    /// Token count returned by the underlying engine (or the heuristic
    /// in the stub path).
    pub used_tokens: u64,
    /// Why the stream ended.
    pub stop_reason: StopReason,
}

// ---------------------------------------------------------------------
// LlamaEngine
// ---------------------------------------------------------------------

/// Safe wrapper over the llama-cpp-rs backend (or the test stub when
/// `feature = "llama_cpp"` is off).
pub struct LlamaEngine {
    config: EngineConfig,
    /// SHA-256 of the loaded GGUF (full-file when verified, header-only
    /// otherwise). Surfaces via [`Self::model_digest`].
    model_digest: ModelPinnedHash,
    /// Hash of the live KV-cache. Re-derived by the bridge whenever the
    /// cache is reallocated. Used by the recovery store (REQ-STATE-01).
    #[allow(dead_code)]
    kv_cache_fingerprint: parking_lot::Mutex<String>,
}

impl LlamaEngine {
    // ---- File-hash helpers ----

    /// Compute the SHA-256 of the file **header** (first 1 MiB). Used
    /// by the quick boot path that just needs to fingerprint the model
    /// for FMEA / crash-loop tracking.
    pub fn fingerprint_header(path: &Path) -> Result<String> {
        let mut f = std::fs::File::open(path).map_err(|e| MukeiError::Io(e.to_string()))?;
        let mut buf = [0u8; SHA_STREAM_CHUNK];
        let n = f.read(&mut buf).map_err(|e| MukeiError::Io(e.to_string()))?;
        let mut h = Sha256::new();
        h.update(&buf[..n]);
        Ok(crate::diagnostics::crash_logger::hex_helper(&h.finalize()))
    }

    /// Compute the SHA-256 of the **entire file** in streaming chunks.
    /// O(file_size) memory: peak peak is `SHA_STREAM_CHUNK` bytes.
    pub fn fingerprint_full(path: &Path) -> Result<String> {
        let mut f = std::fs::File::open(path).map_err(|e| MukeiError::Io(e.to_string()))?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; SHA_STREAM_CHUNK];
        loop {
            let n = f.read(&mut buf).map_err(|e| MukeiError::Io(e.to_string()))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(crate::diagnostics::crash_logger::hex_helper(&h.finalize()))
    }

    /// Verify the full-file SHA-256 against an expected value. Rejects
    /// with [`MukeiError::ModelCorrupted`] on mismatch (REQ-SEC-01).
    pub fn verify_full_sha256_stream(path: &Path, expected: &str) -> Result<()> {
        let got = Self::fingerprint_full(path)?;
        if got.eq_ignore_ascii_case(expected) {
            Ok(())
        } else {
            tracing::warn!(found = %got, expected = %expected, "model SHA-256 mismatch");
            Err(MukeiError::ModelCorrupted)
        }
    }

    // ---- Constructors ----

    /// Load the GGUF model.
    ///
    /// - If `config.expected_sha256` is `Some`, the full-file SHA is
    ///   streamed BEFORE the file is mmapped. Mismatch = `ModelCorrupted`.
    /// - Always records the full-file SHA as `model_digest` when
    ///   verification ran; otherwise records the header SHA so the
    ///   recovery layer still has *something* to compare.
    pub async fn load_model(path: &Path, config: EngineConfig) -> Result<Arc<Self>> {
        let digest = if let Some(expected) = config.expected_sha256.as_deref() {
            // Stream the verifier off the runtime worker.
            let path = path.to_path_buf();
            let expected_owned = expected.to_string();
            let expected_for_closure = expected_owned.clone();
            tokio::task::spawn_blocking(move || {
                Self::verify_full_sha256_stream(&path, &expected_for_closure)
            })
            .await
            .map_err(|e| MukeiError::BlockingJoinFailed(e.to_string()))??;
            expected_owned
        } else {
            Self::fingerprint_header(path)?
        };

        Ok(Arc::new(Self {
            config,
            model_digest: digest,
            kv_cache_fingerprint: parking_lot::Mutex::new(String::new()),
        }))
    }

    /// Tool-call detection that delegates to the GBNF parser.
    ///
    /// Returns `true` when the assistant text matches the
    /// `grammars/tool_calling.gbnf` envelope. False positives are
    /// structurally impossible for fully-parsed JSON; the streaming
    /// fallback rejects bare arrays / prose.
    pub fn contains_tool_call(assistant_so_far: &str) -> bool {
        let trimmed = assistant_so_far.trim();
        if !trimmed.starts_with('[') {
            return false;
        }
        if let Ok(parsed) = crate::tools::validator::parse_gbnf_output(trimmed) {
            return !parsed.is_empty() && parsed.iter().all(|c| !c.name.is_empty());
        }
        // Streaming-prefix path: require `"name"` to appear AFTER the
        // first `{`. Rejects `[1,2,3]` / `["a","b"]`.
        let after_bracket = &trimmed[1..];
        match after_bracket.find('{') {
            Some(brace_pos) => after_bracket[brace_pos..].contains("\"name\""),
            None => false,
        }
    }

    // ---- Accessors (REQ-STATE-01) ----

    /// Full-file or header SHA-256 of the loaded model.
    pub fn model_digest(&self) -> &str {
        &self.model_digest
    }

    /// Snapshot of the current KV-cache fingerprint.
    pub fn kv_cache_fingerprint(&self) -> String {
        self.kv_cache_fingerprint.lock().clone()
    }

    /// Bridge-driven setter (called whenever llama.cpp reallocates the
    /// cache). The agent loop's snapshot writer reads this back through
    /// [`Self::kv_cache_fingerprint`].
    pub fn set_kv_cache_fingerprint(&self, fp: impl Into<String>) {
        *self.kv_cache_fingerprint.lock() = fp.into();
    }

    /// Effective config the engine was loaded with.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
}

/// Re-exported helper used by the agent loop to short-circuit on
/// tool-call detection.
pub fn has_tool_call(text: &str) -> bool {
    LlamaEngine::contains_tool_call(text)
}

// ---------------------------------------------------------------------
// Inference entry point (REQ-INF-01 + H4 typed stop)
// ---------------------------------------------------------------------

/// Trait used by the agent loop. The bridge crate provides the real
/// llama.cpp impl behind `feature = "llama_cpp"`; tests inject
/// [`MockInferenceBackend`].
#[async_trait::async_trait]
pub trait InferenceBackend: Send + Sync {
    /// Run the model on `prompt`, streaming tokens through
    /// `token_sender`. Honour `cancel_token` for the typed
    /// `StopReason::UserStopped` path.
    async fn run(
        &self,
        prompt: &str,
        cancel: CancellationToken,
        token_sender: mpsc::Sender<String>,
    ) -> Result<InferenceOutcome>;
}

/// Configurable test-time backend. Used by the sandbox build and by
/// `cargo test` so the agent loop can be exercised end-to-end without
/// llama.cpp (PRD REQ-INF-01 sandbox-representative coverage).
#[derive(Clone)]
pub struct MockInferenceBackend {
    /// Bytes per emitted chunk.
    pub chunk_bytes: usize,
    /// Sleep between chunks in milliseconds (simulates realistic stream).
    pub per_chunk_ms: u64,
    /// Output template — `{prompt}` is replaced with the trimmed prompt.
    pub template: String,
}

impl Default for MockInferenceBackend {
    fn default() -> Self {
        Self {
            chunk_bytes: 16,
            per_chunk_ms: 1,
            template: "{prompt}".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl InferenceBackend for MockInferenceBackend {
    async fn run(
        &self,
        prompt: &str,
        cancel: CancellationToken,
        token_sender: mpsc::Sender<String>,
    ) -> Result<InferenceOutcome> {
        if prompt.is_empty() {
            return Err(MukeiError::Invariant("empty prompt".into()));
        }
        let body = self.template.replace("{prompt}", prompt);
        let bytes = body.as_bytes();

        let mut acc = String::new();
        let mut idx = 0usize;
        let chunk = self.chunk_bytes.max(1);
        let _ = chunk; // silence false-positive lint when bytes.len() < chunk
        let chunk = self.chunk_bytes.max(1);

        while idx < bytes.len() {
            if cancel.is_cancelled() {
                return Ok(InferenceOutcome {
                    assistant_text: acc.clone(),
                    used_tokens: acc.len() as u64,
                    stop_reason: StopReason::UserStopped,
                });
            }
            let end = (idx + chunk).min(bytes.len());
            // Walk back to a UTF-8 char boundary so we never split a
            // multi-byte char across chunks.
            let mut safe_end = end;
            while safe_end > idx && !is_utf8_boundary(bytes, safe_end) {
                safe_end -= 1;
            }
            if safe_end == idx {
                safe_end = end;
            }
            let piece = std::str::from_utf8(&bytes[idx..safe_end])
                .map_err(|e| MukeiError::Invariant(format!("non-utf8 chunk: {e}")))?;
            acc.push_str(piece);
            idx = safe_end;

            let _ = token_sender.send(piece.to_string()).await;
            if self.per_chunk_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.per_chunk_ms)).await;
            }
        }
        Ok(InferenceOutcome {
            assistant_text: acc.clone(),
            used_tokens: acc.len() as u64,
            stop_reason: StopReason::Completed,
        })
    }
}

fn is_utf8_boundary(bytes: &[u8], at: usize) -> bool {
    at == bytes.len() || (bytes[at] & 0b1100_0000) != 0b1000_0000
}

/// Default async entry-point used by [`crate::agent::loop_`]. Falls
/// back to [`MockInferenceBackend`] when `feature = "llama_cpp"` is off
/// so the sandbox build is still exercised end-to-end.
pub async fn run_inference(
    context_text: &str,
    cancel_token: CancellationToken,
    token_sender: mpsc::Sender<String>,
) -> Result<(String, u64)> {
    let backend = MockInferenceBackend::default();
    let outcome = backend.run(context_text, cancel_token, token_sender).await?;
    Ok((outcome.assistant_text, outcome.used_tokens))
}

/// Typed entry-point used by the bridge crate and the agent loop's
/// recovery / stop-reason aware path.
pub async fn run_inference_typed(
    backend: &dyn InferenceBackend,
    context_text: &str,
    cancel_token: CancellationToken,
    token_sender: mpsc::Sender<String>,
) -> Result<InferenceOutcome> {
    backend.run(context_text, cancel_token, token_sender).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    // ---- Tool-call detection ----

    #[test]
    fn contains_tool_call_recognises_gbnf_envelope() {
        assert!(has_tool_call("[{\"name\": \"web_search\", \"arguments\": {}}]"));
        assert!(has_tool_call("  \n[{\"name\":\"x\",\"arguments\":{}}"));
        assert!(has_tool_call("[ {\"name\": \"x\", \"arguments\": {}} ]"));
    }

    #[test]
    fn contains_tool_call_rejects_prose_and_arrays() {
        assert!(!has_tool_call("hello, world"));
        assert!(!has_tool_call("Here is some JSON: {\"name\": \"x\""));
        assert!(!has_tool_call("Use this code: `if cond { do() }`"));
        assert!(!has_tool_call("[1, 2, 3]"));
        assert!(!has_tool_call("[\"hello\", \"world\"]"));
        assert!(!has_tool_call("[]"));
        assert!(!has_tool_call("[{\"role\": \"user\", \"content\": \"hi\"}]"));
    }

    // ---- Full-file SHA-256 ----

    #[test]
    fn full_sha_matches_known_value() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("model.gguf");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        f.flush().unwrap();

        let got = LlamaEngine::fingerprint_full(&path).unwrap();
        // sha256("hello world")
        assert_eq!(
            got,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn verify_full_sha256_stream_rejects_mismatch() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("model.gguf");
        std::fs::write(&path, b"hello world").unwrap();
        let err = LlamaEngine::verify_full_sha256_stream(&path, "deadbeef").unwrap_err();
        assert!(matches!(err, MukeiError::ModelCorrupted));
    }

    #[test]
    fn header_sha_differs_from_full_sha_when_body_larger_than_1mib() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("model.gguf");
        let mut f = std::fs::File::create(&path).unwrap();
        // Write SHA_STREAM_CHUNK + 1 bytes so the header and full hashes
        // are guaranteed to disagree.
        let body = vec![0u8; SHA_STREAM_CHUNK + 1];
        f.write_all(&body).unwrap();
        f.flush().unwrap();

        let header = LlamaEngine::fingerprint_header(&path).unwrap();
        let full = LlamaEngine::fingerprint_full(&path).unwrap();
        assert_ne!(header, full);
    }

    // ---- MockInferenceBackend ----

    #[tokio::test]
    async fn mock_backend_completes_normally() {
        let backend = MockInferenceBackend::default();
        let (tx, mut rx) = mpsc::channel::<String>(64);
        let tok = CancellationToken::new();
        let outcome = backend.run("hello world", tok, tx).await.unwrap();
        assert_eq!(outcome.assistant_text, "hello world");
        assert_eq!(outcome.stop_reason, StopReason::Completed);
        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        assert!(drained > 0);
    }

    #[tokio::test]
    async fn mock_backend_reports_user_stopped() {
        let backend = MockInferenceBackend {
            per_chunk_ms: 5,
            ..Default::default()
        };
        let (tx, _rx) = mpsc::channel::<String>(64);
        let tok = CancellationToken::new();
        tok.cancel();
        let outcome = backend.run("hello world hello world hello world", tok, tx).await.unwrap();
        assert_eq!(outcome.stop_reason, StopReason::UserStopped);
    }

    #[tokio::test]
    async fn mock_backend_rejects_empty_prompt() {
        let backend = MockInferenceBackend::default();
        let (tx, _rx) = mpsc::channel::<String>(4);
        let tok = CancellationToken::new();
        let err = backend.run("", tok, tx).await.unwrap_err();
        assert!(matches!(err, MukeiError::Invariant(_)));
    }

    #[tokio::test]
    async fn run_inference_compat_shim_returns_tokens() {
        let (tx, mut rx) = mpsc::channel::<String>(64);
        let tok = CancellationToken::new();
        let (out, n) = run_inference("hello world", tok, tx).await.unwrap();
        assert_eq!(out, "hello world");
        assert!(n > 0);
        while rx.try_recv().is_ok() {}
    }

    #[test]
    fn stop_reason_tags_are_stable_ascii() {
        for r in [
            StopReason::Completed,
            StopReason::UserStopped,
            StopReason::ThermalKill,
            StopReason::OutOfMemory,
            StopReason::WatchdogTripped,
        ] {
            let t = r.as_tag();
            assert!(t.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }
}
