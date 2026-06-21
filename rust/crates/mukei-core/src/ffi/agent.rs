//! FFI-friendly snapshot of the agent state machine (TRD §5, §7.0).
//!
//! QML only sees this struct — the bridge crate marshals signals from
//! each variant.  State strings are stable on purpose so the QML switch
//! can be exhaustive over a small enum.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FfiAgentSnapshot {
    /// App launched, Rust core allocating memory, Qt loading QML (§5.1).
    Uninitialized,
    /// GGUF file not found or SHA256 mismatch (§5.1).
    ModelMissing { expected_sha256: String },
    /// Resumable, chunked download with SHA256 streaming verification.
    Downloading { bytes_so_far: u64, bytes_total: u64 },
    /// GGUF mapped to memory, KV-Cache allocated, Tokenizer parsed.
    Loading { stage: LoadingStage },
    /// Model in RAM, waiting for user input.
    IdleReady    { model_alias: String },
    /// LLM generating tokens.
    Inferring    { tokens_generated: u32 },
    /// LLM paused, Rust executing external tasks (Web, File I/O).
    ToolExecuting { tool: String },
    /// App resumed from OS background kill, rebuilding state.
    Recovering   { last_token_index: u32 },
    /// Generation paused or context reduced due to heat.
    ThermalThrottled { so_c_status: u8 },
    /// Unrecoverable hardware or corruption state.
    FatalError   { code: String },
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadingStage {
    ReadingGguf,
    AllocatingKvCache,
    ParsingTokenizer,
    WarmingUp,
}
