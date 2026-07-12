//! `mukei_core::engine` — TRD §3 / PRD §9.
//!
//! Modules:
//! - [`tokenizer`] — token counter (heuristic + real BPE).
//! - [`llama_wrapper`] — `LlamaEngine`, `EngineConfig`,
//!   `InferenceBackend`, `MockInferenceBackend`, `StopReason`, full-file
//!   SHA verification, GBNF-aware tool-call detection.
//! - [`gpu_strategy`] — Mali / Adreno / Sugarloaf detection with
//!   thermal-aware fallback.
//! - [`streaming`] — 50 ms-batched token drain from raw mpsc.
//! - [`markdown`] — pre-typed AST serializer for QML.
//! - [`model_registry`] — canonical Gemma 4 E2B / E4B catalogue +
//!   device-tier picker used by the bridge `download_model` flow
//!   (TRD §8.1 / REQ-MOD-01).

pub mod activation;
pub mod gpu_strategy;
pub mod llama_wrapper;
pub mod markdown;
pub mod model_registry;
pub mod streaming;
pub mod tokenizer;

pub use activation::{
    ActivationCommit, ActivationFailureCategory, InferenceBackendFactory,
    InferenceReadinessSnapshot, ModelActivationService, ModelActivationState,
    VerifiedModelArtifact, VerifiedModelDescriptor,
};
pub use gpu_strategy::{GpuKind, GpuStrategy};
pub use llama_wrapper::{
    has_tool_call, run_inference, run_inference_typed, run_inference_with_mock_for_tests,
    BackendIdentity, BackendKind, BackendUnavailableReason, EngineConfig, InferenceBackend,
    InferenceOutcome, LlamaEngine, MockInferenceBackend, ModelPinnedHash, StopReason,
    UnavailableInferenceBackend,
};
pub use model_registry::{
    lookup as lookup_model, lookup_str as lookup_model_str, recommended_for_device,
    ModelDescriptor, ModelId, MODELS,
};
pub use streaming::{Drainer, TokenStreamConfig};
#[cfg(feature = "candle")]
pub use tokenizer::BpeTokenizer;
pub use tokenizer::{CharCountTokenizer, TokenCount};
