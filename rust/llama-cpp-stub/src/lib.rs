//! Placeholder for the real `llama-cpp-rs` crate. The bridge build
//! replaces this path-dep with the upstream git rev at native build time
//! (see TRD §8.2 + llama-cpp-prebuilt/CMakeLists.txt).
//!
//! The stub exposes JUST enough API surface for `mukei-core` to compile
//! when the `llama_cpp` feature flag is enabled in unit tests / sandbox
//! builds.

use std::fmt;

#[derive(Debug)]
pub enum LlamaError {
    StubOnly,
}

impl fmt::Display for LlamaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlamaError::StubOnly => write!(f, "stub: feature `llama_cpp` requires a real llama.cpp build"),
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
    pub fn load(_path: &str, _params: LlamaParams) -> Result<Self, LlamaError> { Err(LlamaError::StubOnly) }
    pub fn create_context(&self) -> Result<LlamaContext, LlamaError> { Err(LlamaError::StubOnly) }
}

impl GbnfGrammar {
    pub fn from_file(_path: &str) -> Result<Self, LlamaError> { Err(LlamaError::StubOnly) }
    pub fn from_string(_s: &str) -> Result<Self, LlamaError> { Err(LlamaError::StubOnly) }
}
