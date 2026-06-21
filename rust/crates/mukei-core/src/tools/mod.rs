//! `mukei_core::tools` — TRD §5 and §13.3.
//!
//! Registry and execution surface for all LLM-callable tools.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::{MukeiError, Result};

pub mod file_tool;
pub mod hardware;
pub mod math;
pub mod validator;
pub mod web_search;

pub const MAX_FAILURES_PER_TOOL: usize = 2;
pub const ALLOWED_TOOLS: &[&str] = &[
    "web_search",
    "read_file",
    "get_hardware_info",
    "math_eval",
];

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
        registry.register(hardware::HardwareTool::default());
        registry.register(math::MathTool::default());
        registry
    }

    pub fn with_file_tool(file_tool: file_tool::FileTool) -> Self {
        let mut registry = Self::new();
        registry.register(file_tool);
        registry
    }

    pub fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        self.inner
            .insert(tool.name().to_string(), Arc::new(tool));
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
        assert_eq!(names, vec![
            "get_hardware_info".to_string(),
            "math_eval".to_string(),
            "read_file".to_string(),
            "web_search".to_string(),
        ]);
    }
}
