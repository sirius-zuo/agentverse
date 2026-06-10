use agentverse::{ErasedTool, ToolCall, ToolCallResult, ToolError, ToolResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::find_tools::FindToolsTool;
use crate::search::{BM25Index, ToolInfo};

type ToolEntry = (Arc<dyn ErasedTool>, ToolOptions);
type ToolMap = HashMap<String, ToolEntry>;

#[derive(Default, Clone)]
pub struct ToolOptions {
    pub category: Option<String>,
    pub execution_mode: ExecutionMode,
}

#[derive(Default, Clone, PartialEq)]
pub enum ExecutionMode {
    #[default]
    Inline,
    Background,
}

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

pub struct ToolRegistry {
    tools: RwLock<ToolMap>,
    index: RwLock<BM25Index>,
}

impl ToolRegistry {
    /// Create a registry with `find_tools` pre-registered.
    pub fn new() -> Arc<Self> {
        let registry = Arc::new(Self {
            tools: RwLock::new(HashMap::new()),
            index: RwLock::new(BM25Index::new()),
        });
        registry.register(FindToolsTool::new(Arc::clone(&registry)));
        registry
    }

    /// Register a tool with default options.
    pub fn register<T: agentverse::Tool + 'static>(&self, tool: T) {
        self.register_with_options(tool, ToolOptions::default());
    }

    /// Register a tool with explicit category and execution mode.
    pub fn register_with_options<T: agentverse::Tool + 'static>(&self, tool: T, opts: ToolOptions) {
        let name = tool.name().to_string();
        let text = format!("{} {}", tool.name(), tool.description());
        let erased: Arc<dyn ErasedTool> = Arc::new(tool);
        self.tools
            .write()
            .unwrap()
            .insert(name.clone(), (erased, opts));
        self.index.write().unwrap().insert(&name, &text);
    }

    /// Register a pre-erased tool (for MCP adapters and other non-Tool implementors).
    pub fn register_erased(&self, tool: Arc<dyn ErasedTool>, opts: ToolOptions) {
        let name = tool.name().to_string();
        let text = format!("{} {}", tool.name(), tool.description());
        self.tools
            .write()
            .unwrap()
            .insert(name.clone(), (tool, opts));
        self.index.write().unwrap().insert(&name, &text);
    }

    /// Execute one tool by name, dispatching its JSON args.
    pub async fn execute(&self, name: &str, args: Value) -> ToolResult {
        let tool = {
            let tools = self.tools.read().unwrap();
            tools.get(name).map(|(t, _)| Arc::clone(t))
        };
        let tool = tool.ok_or_else(|| {
            tracing::warn!(tool_name = %name, "Tool not found");
            ToolError::NotFound(name.to_string())
        })?;
        tracing::debug!(tool_name = %name, args = %safe_truncate(&args.to_string(), 200), "Tool call");
        let result = tool.execute_raw(args).await;
        match &result {
            Ok(v) => {
                tracing::info!(tool_name = %name, result = %safe_truncate(&v.to_string(), 200), "Tool ok")
            }
            Err(e) => tracing::warn!(tool_name = %name, error = %e, "Tool error"),
        }
        result
    }

    /// Execute multiple tool calls concurrently. Results are in completion order.
    pub async fn execute_many(&self, calls: Vec<ToolCall>) -> Vec<ToolCallResult> {
        let resolved: Vec<(Option<Arc<dyn ErasedTool>>, ToolCall)> = {
            let tools = self.tools.read().unwrap();
            calls
                .into_iter()
                .map(|c| {
                    let t = tools.get(&c.name).map(|(t, _)| Arc::clone(t));
                    (t, c)
                })
                .collect()
        };

        let mut set = tokio::task::JoinSet::new();
        for (tool_opt, c) in resolved {
            set.spawn(async move {
                let result = match tool_opt {
                    Some(t) => t.execute_raw(c.args).await,
                    None => Err(ToolError::NotFound(c.name.clone())),
                };
                ToolCallResult {
                    name: c.name,
                    result,
                }
            });
        }

        let mut results = Vec::new();
        while let Some(Ok(r)) = set.join_next().await {
            results.push(r);
        }
        results
    }

    /// Spawn a tool fire-and-forget; returns a handle to await later.
    pub fn spawn_tool(&self, call: ToolCall) -> agentverse::ToolHandle {
        let (tx, rx) = tokio::sync::oneshot::channel::<ToolCallResult>();
        let tool_opt = {
            let tools = self.tools.read().unwrap();
            tools.get(&call.name).map(|(t, _)| Arc::clone(t))
        };
        tokio::spawn(async move {
            let result = match tool_opt {
                Some(t) => t.execute_raw(call.args).await,
                None => Err(ToolError::NotFound(call.name.clone())),
            };
            let _ = tx.send(ToolCallResult {
                name: call.name,
                result,
            });
        });
        agentverse::ToolHandle {
            id: uuid::Uuid::new_v4(),
            receiver: rx,
        }
    }

    /// Return Anthropic-compatible tool definitions for all registered tools.
    pub fn schema(&self) -> Vec<Value> {
        self.tools
            .read()
            .unwrap()
            .values()
            .map(|(t, _)| t.schema())
            .collect()
    }

    /// BM25 keyword search over tool names and descriptions.
    pub fn search(&self, query: &str, limit: usize) -> Vec<ToolInfo> {
        let hits = self.index.read().unwrap().search(query, limit);
        let tools = self.tools.read().unwrap();
        hits.into_iter()
            .filter_map(|(name, score)| {
                tools.get(&name).map(|(t, _)| ToolInfo {
                    name: name.clone(),
                    description: t.description().to_string(),
                    schema: t.schema(),
                    score,
                })
            })
            .collect()
    }

    /// Return a new registry containing only tools with the given category.
    pub fn filter_category(&self, category: &str) -> Arc<ToolRegistry> {
        let new_reg = Arc::new(ToolRegistry {
            tools: RwLock::new(HashMap::new()),
            index: RwLock::new(BM25Index::new()),
        });
        let tools = self.tools.read().unwrap();
        for (name, (tool, opts)) in tools.iter() {
            if opts.category.as_deref() == Some(category) {
                new_reg
                    .tools
                    .write()
                    .unwrap()
                    .insert(name.clone(), (Arc::clone(tool), opts.clone()));
                let text = format!("{} {}", tool.name(), tool.description());
                new_reg.index.write().unwrap().insert(name, &text);
            }
        }
        new_reg
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.read().unwrap().contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.tools.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.read().unwrap().is_empty()
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tools.read().unwrap().keys().cloned().collect()
    }

    /// Human-readable tool summary for ReAct prompt injection.
    pub fn tool_summaries(&self) -> String {
        let tools = self.tools.read().unwrap();
        if tools.is_empty() {
            return "none (reasoning only)".to_string();
        }
        let mut entries: Vec<String> = tools
            .values()
            .map(|(t, _)| {
                let schema = t.schema();
                let props = schema["input_schema"]["properties"].as_object();
                let required: Vec<&str> = schema["input_schema"]["required"]
                    .as_array()
                    .map(|r| r.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
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

    /// Human-readable tool summary for a named subset of tools.
    /// Returns "none (reasoning only)" if `names` is empty or no named tools are registered.
    pub fn tool_summaries_for(&self, names: &[String]) -> String {
        if names.is_empty() {
            return "none (reasoning only)".to_string();
        }
        let tools = self.tools.read().unwrap();
        let mut entries: Vec<String> = tools
            .iter()
            .filter(|(name, _)| names.contains(name))
            .map(|(_, (t, _))| {
                let schema = t.schema();
                let props = schema["input_schema"]["properties"].as_object();
                let required: Vec<&str> = schema["input_schema"]["required"]
                    .as_array()
                    .map(|r| r.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let args_hint = if let Some(props) = props {
                    let fields: Vec<String> = required
                        .iter()
                        .filter_map(|k| {
                            props.get(*k).map(|v| {
                                let desc =
                                    v.get("description").and_then(|d| d.as_str()).unwrap_or("");
                                format!("\"{k}: \"{desc}\"")
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
        if entries.is_empty() {
            return "none (reasoning only)".to_string();
        }
        entries.sort();
        entries.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_summaries_for_empty_names_returns_none_string() {
        let reg = ToolRegistry::new();
        assert_eq!(reg.tool_summaries_for(&[]), "none (reasoning only)");
    }

    #[test]
    fn tool_summaries_for_unknown_names_returns_none_string() {
        let reg = ToolRegistry::new();
        assert_eq!(
            reg.tool_summaries_for(&["ghost".to_string()]),
            "none (reasoning only)"
        );
    }
}
