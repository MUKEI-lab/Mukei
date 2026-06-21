//! `mukei_core::engine` — TRD §3.
//!
//! The Rust half of the `llama-cpp-rs` wrapper. The crate builds
//! against either the precompiled `llama-cpp-sys` archive (TRD §8.2
//! — preferred, no per-PR 30-min CI rebuild) or against a stub
//! `InferenceBackend` trait for unit tests.
//!
//! Modules:
//!   - `tokenizer`       — token counter shared with the agent loop.
//!   - `llama_wrapper`   — `LlamaEngine` stub + streaming entry point.
//!   - `gpu_strategy`    — Mali / Adreno layer-splitting heuristic.
//!   - `streaming`       — 50 ms-batched token drain from raw mpsc.
//!   - `markdown`        — pre-typed AST serializer for QML (TRD §35.1.1).

pub mod gpu_strategy;
pub mod llama_wrapper;
pub mod markdown;
pub mod streaming;
pub mod tokenizer;

pub use llama_wrapper::{run_inference, has_tool_call};
pub use streaming::{Drainer, TokenStreamConfig};
pub use tokenizer::{CharCountTokenizer, TokenCount};
