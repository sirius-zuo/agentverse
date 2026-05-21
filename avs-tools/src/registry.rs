use agentverse::{AsyncTool, ToolError, ToolResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

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
        let (tool, _) = self.tools.get(name).ok_or_else(|| {
            tracing::warn!(tool_name = %name, "Tool not found");
            ToolError::NotFound(name.to_string())
        })?;

        let args_str = args.to_string();
        tracing::debug!(tool_name = %name, tool_args = %safe_truncate(&args_str, 200), "Tool call");

        let result = tool.execute(args).await;

        match &result {
            Ok(v) => {
                let res_str = v.to_string();
                tracing::info!(tool_name = %name, tool_result = %safe_truncate(&res_str, 200), "Tool executed");
            }
            Err(e) => {
                tracing::warn!(tool_name = %name, error = %e, "Tool error");
            }
        }
        result
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

    /// Return a human-readable summary of every tool with its required args,
    /// suitable for injecting into a planner system prompt.
    pub fn tool_summaries(&self) -> String {
        if self.tools.is_empty() {
            return "none (reasoning only)".to_string();
        }
        let mut entries: Vec<String> = self
            .tools
            .values()
            .map(|(t, _)| {
                let params = t.parameters();
                let required: Vec<&str> = params
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let props = params.get("properties").and_then(|p| p.as_object());
                let args_hint = if let Some(props) = props {
                    let fields: Vec<String> = required
                        .iter()
                        .filter_map(|k| {
                            props.get(*k).map(|v| {
                                let desc =
                                    v.get("description").and_then(|d| d.as_str()).unwrap_or("");
                                format!("\"{k}\": \"{desc}\"")
                            })
                        })
                        .collect();
                    format!("{{{}}}", fields.join(", "))
                } else {
                    "{}".to_string()
                };
                format!("- {}: {}\n  args: {}", t.name(), t.description(), args_hint)
            })
            .collect();
        entries.sort();
        entries.join("\n")
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

#[cfg(test)]
mod logging_tests {
    use super::*;
    use agentverse::{AsyncTool, ToolResult};
    use serde_json::json;

    struct EchoTool;

    #[async_trait::async_trait]
    impl AsyncTool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echoes input"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({"type":"object","properties":{"msg":{"type":"string"}},"required":["msg"]})
        }
        async fn execute(&self, args: serde_json::Value) -> ToolResult {
            Ok(args["msg"].as_str().unwrap_or("").to_string().into())
        }
    }

    #[tokio::test]
    async fn execute_logs_without_panic() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);
        let result = registry.execute("echo", json!({"msg": "hello"})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn execute_unknown_tool_logs_error() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let registry = ToolRegistry::new();
        let result = registry.execute("nonexistent", json!({})).await;
        assert!(result.is_err());
    }
}
