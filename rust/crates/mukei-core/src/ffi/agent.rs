//! FFI-friendly snapshot of the agent state machine (TRD §5, §7.0).
//!
//! QML only sees this struct — the bridge crate marshals signals from
//! each variant.  State strings are stable on purpose so the QML switch
//! can be exhaustive over a small enum.

use serde::{Deserialize, Serialize};

/// Stable JSON snapshot of the agent's state machine. QML treats this
/// as the single source of truth for screen routing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FfiAgentSnapshot {
    /// App launched, Rust core allocating memory, Qt loading QML (§5.1).
    Uninitialized,
    /// GGUF file not found or SHA256 mismatch (§5.1).
    ModelMissing {
        /// SHA-256 the file should match before mmap is allowed.
        expected_sha256: String,
    },
    /// Resumable, chunked download with SHA256 streaming verification.
    Downloading {
        /// Bytes durably written to the `.part` file so far.
        bytes_so_far: u64,
        /// Total bytes the download will produce when complete.
        bytes_total: u64,
    },
    /// GGUF mapped to memory, KV-Cache allocated, Tokenizer parsed.
    Loading {
        /// Which sub-stage of the model load is currently active.
        stage: LoadingStage,
    },
    /// Model in RAM, waiting for user input.
    IdleReady {
        /// Display alias of the loaded model (e.g. `"qwen2.5-3b"`).
        model_alias: String,
    },
    /// LLM generating tokens.
    Inferring {
        /// Token count emitted in this turn so far.
        tokens_generated: u32,
    },
    /// LLM paused, Rust executing external tasks (Web, File I/O).
    ToolExecuting {
        /// Tool currently executing.
        tool: String,
    },
    /// App resumed from OS background kill, rebuilding state.
    Recovering {
        /// Last token index successfully persisted before the kill.
        last_token_index: u32,
    },
    /// Generation paused or context reduced due to heat.
    ThermalThrottled {
        /// SoC thermal status code (0–4 per Android API).
        so_c_status: u8,
    },
    /// Unrecoverable hardware or corruption state.
    FatalError {
        /// Stable `ERR_*` code from [`crate::error::MukeiError::error_code`].
        code: String,
    },
}

/// Sub-stage of [`FfiAgentSnapshot::Loading`] — lets QML render a
/// per-stage progress label rather than a single opaque "loading…".
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadingStage {
    /// Reading and parsing the GGUF header.
    ReadingGguf,
    /// Allocating the KV cache (largest single allocation).
    AllocatingKvCache,
    /// Parsing the SentencePiece / BPE tokenizer.
    ParsingTokenizer,
    /// Running a dummy forward pass to warm the cache.
    WarmingUp,
}
