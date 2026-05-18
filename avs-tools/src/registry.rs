use agentverse::{AsyncTool, ToolError, ToolResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of available async tools, optionally tagged with a category.
///
/// Internal storage uses `Arc<dyn AsyncTool>` so `filter_category` can
/// produce a new registry sharing the same tool instances without cloning.
pub struct ToolRegistry {
    tools: HashMap<String, (Arc<dyn AsyncTool>, Option<String>)>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool with no category.
    pub fn register<T: AsyncTool + 'static>(&mut self, tool: T) {
        self.tools
            .insert(tool.name().to_string(), (Arc::new(tool), None));
    }

    /// Register a tool with a category tag (e.g. "shell", "network", "math").
    pub fn register_with_category<T: AsyncTool + 'static>(&mut self, tool: T, category: &str) {
        self.tools.insert(
            tool.name().to_string(),
            (Arc::new(tool), Some(category.to_string())),
        );
    }

    /// Return a new registry containing only tools with the given category.
    pub fn filter_category(&self, category: &str) -> ToolRegistry {
        let tools = self
            .tools
            .iter()
            .filter(|(_, (_, cat))| cat.as_deref() == Some(category))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        ToolRegistry { tools }
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, args: Value) -> ToolResult {
        let (tool, _) = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.execute(args).await
    }

    /// Return JSON schema objects for all registered tools (for prompt injection).
    pub fn schema(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|(t, _)| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters(),
                })
            })
            .collect()
    }

    /// Iterate over all registered tools.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn AsyncTool>> {
        self.tools.values().map(|(t, _)| t)
    }

    /// Return all registered tool names.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Check if a tool is registered.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Return the number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Return true if no tools are registered.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
