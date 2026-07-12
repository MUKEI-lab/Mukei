//! `mukei_core::tools` — TRD §5 and §13.3.
//!
//! Registry and execution surface for all LLM-callable tools.
//!
//! # Invariants
//!
//! - The tool name set is **closed**: every entry in [`ALLOWED_TOOLS`]
//!   has a `validator.rs` schema, an entry in `grammars/tool_calling.gbnf`,
//!   and a registered [`Tool`] impl. Adding a tool requires touching all
//!   three; otherwise the GBNF can emit a name the validator rejects or
//!   vice versa.
//! - The validator runs BEFORE the executor. No tool sees raw LLM JSON.
//! - Every tool's `run()` MUST acquire one slot of
//!   [`crate::runtime::TOOL_SLOTS`] (via `runtime::spawn_blocking_tool`)
//!   before doing blocking work. This caps total concurrent blocking
//!   tool work at [`crate::runtime::TOOL_BLOCKING_SLOTS`] regardless of
//!   how many tools the LLM emits in one batch (TRD §2.2).
//! - Tool output crossing back to the LLM MUST be wrapped in
//!   `<external_data source="..." trust="...">` so prompt injection
//!   from web pages / files cannot impersonate system instructions.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::error::{MukeiError, Result};

pub mod file_tool;
pub mod hardware;
pub mod math;
pub mod permission;
pub mod remote_policy;
pub mod sentinel;
pub mod validator;
pub mod web_search;

pub use remote_policy::RemoteFeaturePolicy;

// NOTE: the `MAX_FAILURES_PER_TOOL` constant that used to live here held a
// stale value (`2`) that disagreed with the audit-recommended threshold
// (`5`) defined by [`crate::agent::tools::ToolExecutionPolicy`]. It was
// deleted in Issue #18 to remove the grep-trap. Use
// `ToolExecutionPolicy::max_failures_per_tool` instead.
pub const ALLOWED_TOOLS: &[&str] = &["web_search", "read_file", "get_hardware_info", "math_eval"];

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(&self, arguments: Value) -> Result<String>;
}

pub struct ToolRegistry {
    inner: HashMap<String, Arc<dyn Tool>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            inner: HashMap::new(),
        };
        registry.register(web_search::WebSearchTool::default());
        registry.register(file_tool::FileTool::default());
        registry.register(hardware::HardwareTool);
        registry.register(math::MathTool);
        registry
    }

    pub fn with_file_tool(file_tool: file_tool::FileTool) -> Self {
        let mut registry = Self::new();
        registry.register(file_tool);
        registry
    }

    /// Bridge entry point: build the registry with a `WebSearchTool`
    /// whose planner has the wrapped-secrets-derived API keys injected
    /// (Issue #3). All other tools are constructed with their defaults.
    pub fn with_web_search_keys(
        brave_key: impl Into<String>,
        tavily_key: impl Into<String>,
    ) -> Self {
        Self::with_web_search_keys_and_policy(brave_key, tavily_key, RemoteFeaturePolicy::default())
    }

    pub fn with_web_search_keys_and_policy(
        brave_key: impl Into<String>,
        tavily_key: impl Into<String>,
        remote_policy: RemoteFeaturePolicy,
    ) -> Self {
        Self::with_web_search_secrets_and_policy(
            Zeroizing::new(brave_key.into()),
            Zeroizing::new(tavily_key.into()),
            remote_policy,
        )
    }

    /// Bridge-oriented constructor that keeps provider credentials in
    /// zeroizing owners throughout the registry rebuild.
    pub fn with_web_search_secrets_and_policy(
        brave_key: Zeroizing<String>,
        tavily_key: Zeroizing<String>,
        remote_policy: RemoteFeaturePolicy,
    ) -> Self {
        let mut registry = Self {
            inner: HashMap::new(),
        };
        registry.register(web_search::WebSearchTool::with_secret_keys_and_policy(
            brave_key,
            tavily_key,
            remote_policy,
        ));
        registry.register(file_tool::FileTool::default());
        registry.register(hardware::HardwareTool);
        registry.register(math::MathTool);
        registry
    }

    pub fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        self.inner.insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.inner.get(name).cloned()
    }

    pub fn require(&self, name: &str) -> Result<Arc<dyn Tool>> {
        self.get(name).ok_or_else(|| MukeiError::UnknownTool {
            tool_name: name.to_string(),
        })
    }

    pub fn names(&self) -> Vec<String> {
        let mut names = self.inner.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_all_tools() {
        let names = ToolRegistry::new().names();
        assert_eq!(
            names,
            vec![
                "get_hardware_info".to_string(),
                "math_eval".to_string(),
                "read_file".to_string(),
                "web_search".to_string(),
            ]
        );
    }
}
