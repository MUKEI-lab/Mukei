//! Placeholder for the real `llama-cpp-rs` crate. The bridge build
//! replaces this path-dep with the upstream git rev at native build time
//! (see TRD §8.2 + llama-cpp-prebuilt/CMakeLists.txt).
//!
//! The stub exposes JUST enough API surface for `mukei-core` to compile
//! when the `llama_cpp` feature flag is enabled in unit tests / sandbox
//! builds.
//!
//! # Release-hardening tripwire (Architect review GH #4)
//!
//! Without the compile-time guard below, a release build that *also*
//! enables `llama_cpp` would silently link this stub and ship a
//! LLM-less agent. The guard is symmetric to the existing `cfg(ddg)`
//! tripwire in `mukei-core::search`: it forces a real binding swap at
//! release time.
//!
//! Tests and sandbox builds opt in to the stub explicitly by passing
//! `--features llama-cpp-rs/stub-acknowledged` (or the workspace alias
//! pre-baked in `cargo test` invocations in CI).
#[cfg(all(feature = "release-hardening", not(feature = "stub-acknowledged"),))]
compile_error!(
    "llama-cpp-stub is being linked into a release-hardening build. \
     Repoint `llama-cpp-rs` in the workspace `[workspace.dependencies]` \
     table at the real llama-cpp-rs git rev (TRD §8.2), or, for \
     intentionally stubbed unit-test builds, enable the \
     `llama-cpp-rs/stub-acknowledged` feature."
);

// The stub also re-exports the `release-hardening` flag from the
// workspace so the cfg above can resolve from this crate's own
// `[features]` table. The actual feature is declared in Cargo.toml.
#[cfg(any(feature = "release-hardening", feature = "stub-acknowledged"))]
const _: () = ();

use std::fmt;

#[derive(Debug)]
pub enum LlamaError {
    StubOnly,
}

impl fmt::Display for LlamaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlamaError::StubOnly => write!(
                f,
                "stub: feature `llama_cpp` requires a real llama.cpp build"
            ),
        }
    }
}

impl std::error::Error for LlamaError {}

#[derive(Debug, Default, Clone)]
pub struct LlamaParams {
    pub n_ctx: usize,
    pub n_threads: usize,
    pub n_gpu_layers: i32,
}

pub struct LlamaModel;
pub struct LlamaContext;
pub struct GbnfGrammar;
pub struct SamplingParams;

impl LlamaModel {
    pub fn load(_path: &str, _params: LlamaParams) -> Result<Self, LlamaError> {
        Err(LlamaError::StubOnly)
    }
    pub fn create_context(&self) -> Result<LlamaContext, LlamaError> {
        Err(LlamaError::StubOnly)
    }
}

impl GbnfGrammar {
    pub fn from_file(_path: &str) -> Result<Self, LlamaError> {
        Err(LlamaError::StubOnly)
    }
    pub fn from_string(_s: &str) -> Result<Self, LlamaError> {
        Err(LlamaError::StubOnly)
    }
}
