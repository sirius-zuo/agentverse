use agentverse::{SyncTool, ToolResult};
use std::collections::HashMap;

/// Registry of available tools.
/// Supports static registration (built-in) and dynamic registration (MCP).
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn SyncTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool by name.
    pub fn register(&mut self, tool: impl SyncTool + 'static) {
        self.tools.insert(tool.name().to_string(), Box::new(tool));
    }

    /// Internal: register a boxed trait object.
    fn register_boxed(&mut self, tool: Box<dyn SyncTool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Register multiple tools from boxed trait objects.
    pub fn register_many(&mut self, tools: Vec<Box<dyn SyncTool>>) {
        for tool in tools {
            self.register_boxed(tool);
        }
    }

    /// Execute a tool by name.
    pub fn execute(&self, tool_name: &str, args: serde_json::Value) -> ToolResult {
        let tool = self.tools.get(tool_name)
            .ok_or_else(|| agentverse::ToolError::NotFound(tool_name.to_string()))?;
        tool.execute(args)
    }

    /// Get all registered tool names.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Check if a tool exists.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
