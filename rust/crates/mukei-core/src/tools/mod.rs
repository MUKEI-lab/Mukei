//! `mukei_core::tools` — validated LLM-callable tool registry.

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
        Self::local_only()
    }
}

impl ToolRegistry {
    /// Safe default registry. Remote tools are absent until credentials and an
    /// explicit remote policy are injected by the secure composition root.
    pub fn new() -> Self {
        Self::local_only()
    }

    pub fn local_only() -> Self {
        let mut registry = Self {
            inner: HashMap::new(),
        };
        registry.register(file_tool::FileTool::default());
        registry.register(hardware::HardwareTool);
        registry.register(math::MathTool);
        registry
    }

    pub fn with_file_tool(file_tool: file_tool::FileTool) -> Self {
        let mut registry = Self::local_only();
        registry.register(file_tool);
        registry
    }

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

    pub fn with_web_search_secrets_and_policy(
        brave_key: Zeroizing<String>,
        tavily_key: Zeroizing<String>,
        remote_policy: RemoteFeaturePolicy,
    ) -> Self {
        let mut registry = Self::local_only();
        registry.register(web_search::WebSearchTool::with_secret_keys_and_policy(
            brave_key,
            tavily_key,
            remote_policy,
        ));
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
    fn default_registry_is_local_only() {
        assert_eq!(
            ToolRegistry::new().names(),
            vec![
                "get_hardware_info".to_string(),
                "math_eval".to_string(),
                "read_file".to_string(),
            ]
        );
    }

    #[test]
    fn web_search_requires_explicit_composition() {
        let registry = ToolRegistry::with_web_search_keys_and_policy(
            "brave",
            "tavily",
            RemoteFeaturePolicy::RemoteAllowed,
        );
        assert!(registry.get("web_search").is_some());
    }
}
