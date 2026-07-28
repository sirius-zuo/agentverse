use agentverse::{ErasedTool, ToolCall, ToolCallResult, ToolError, ToolResult};

/// Carries HITL interrupt info when execute_many_hitl intercepts a call.
pub struct HitlInterruptResult {
    pub approval_id: uuid::Uuid,
    pub kind_json: String,
}
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Tool name used by SubAgentTool. Defined here so both the tool implementation
/// and the registry filter can reference a single authoritative constant.
pub const SPAWN_SUBAGENT_TOOL_NAME: &str = "spawn_subagent";

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

    /// Register a tool only when its name is absent, returning whether it was inserted.
    /// The tool map and search document are updated once under the same map write lock.
    pub fn register_if_absent<T: agentverse::Tool + 'static>(&self, tool: T) -> bool {
        let name = tool.name().to_string();
        let text = format!("{} {}", tool.name(), tool.description());
        let erased: Arc<dyn ErasedTool> = Arc::new(tool);
        let mut tools = self.tools.write().unwrap();

        match tools.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(entry) => {
                self.index.write().unwrap().insert(&name, &text);
                entry.insert((erased, ToolOptions::default()));
                true
            }
        }
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
        let start = std::time::Instant::now();
        let tool = {
            let tools = self.tools.read().unwrap();
            tools.get(name).map(|(t, _)| Arc::clone(t))
        };
        let tool = match tool {
            Some(t) => t,
            None => {
                tracing::warn!(tool_name = %name, "Tool not found");
                agentverse::metrics::record_tool_call(
                    "<unknown>",
                    start.elapsed(),
                    agentverse::metrics::ToolOutcome::Error,
                );
                return Err(ToolError::NotFound(name.to_string()));
            }
        };
        tracing::debug!(tool_name = %name, args = %safe_truncate(&args.to_string(), 200), "Tool call");
        let result = tool.execute_raw(args).await;
        let outcome = match &result {
            Ok(v) => {
                tracing::info!(tool_name = %name, result = %safe_truncate(&v.to_string(), 200), "Tool ok");
                agentverse::metrics::ToolOutcome::Ok
            }
            Err(e) => {
                tracing::warn!(tool_name = %name, error = %e, "Tool error");
                agentverse::metrics::ToolOutcome::Error
            }
        };
        agentverse::metrics::record_tool_call(name, start.elapsed(), outcome);
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
                let start = std::time::Instant::now();
                let not_found = tool_opt.is_none();
                let result = match tool_opt {
                    Some(t) => t.execute_raw(c.args).await,
                    None => Err(ToolError::NotFound(c.name.clone())),
                };
                let outcome = if result.is_ok() {
                    agentverse::metrics::ToolOutcome::Ok
                } else {
                    agentverse::metrics::ToolOutcome::Error
                };
                let metric_name = if not_found { "<unknown>" } else { &c.name };
                agentverse::metrics::record_tool_call(metric_name, start.elapsed(), outcome);
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

    /// Execute tool calls, intercepting any that require HITL approval.
    /// Returns Err(HitlInterruptResult) for the first intercepted call.
    /// Does NOT execute any calls if any one requires approval — all or nothing.
    pub async fn execute_many_hitl(
        &self,
        calls: Vec<ToolCall>,
        hook: &Arc<dyn agentverse::hitl::HitlHook>,
    ) -> Result<Vec<ToolCallResult>, HitlInterruptResult> {
        // Check all calls first — intercept before executing any
        for call in &calls {
            if let Some((approval_id, kind_json)) = hook.check_tool(&call.name, &call.args).await {
                agentverse::metrics::record_tool_call(
                    &call.name,
                    std::time::Duration::ZERO,
                    agentverse::metrics::ToolOutcome::HitlIntercepted,
                );
                return Err(HitlInterruptResult {
                    approval_id,
                    kind_json,
                });
            }
        }
        // No HITL needed — execute normally
        Ok(self.execute_many(calls).await)
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

    /// Return native tool definitions for the requested registered tools, with
    /// each tool's schema run through the strict-mode adapter (see
    /// `crate::strict_schema`). Unknown names are silently skipped, preserving
    /// the order of `names`. Returns `Err` if any requested tool's schema
    /// cannot be made strict-mode compatible (e.g. an open-dictionary shape) —
    /// this is not caught and skipped, since silently dropping a tool from the
    /// list offered to the model is its own kind of silent failure.
    pub fn tool_definitions_for(
        &self,
        names: &[String],
    ) -> Result<Vec<agentverse::ToolDefinition>, crate::strict_schema::StrictSchemaError> {
        let tools = self.tools.read().unwrap();
        names
            .iter()
            .filter_map(|name| tools.get(name))
            .map(|(tool, _)| {
                let schema = tool.schema();
                let parameters =
                    crate::strict_schema::to_strict_schema(schema["input_schema"].clone())?;
                Ok(agentverse::ToolDefinition {
                    name: schema["name"].as_str().unwrap_or_default().to_string(),
                    description: schema["description"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    parameters,
                })
            })
            .collect::<Result<Vec<_>, _>>()
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

    fn copy_named_tools(&self, names: &[String], exclude_subagent_spawner: bool) -> Arc<Self> {
        let new_reg = Arc::new(ToolRegistry {
            tools: RwLock::new(HashMap::new()),
            index: RwLock::new(BM25Index::new()),
        });
        let tools = self.tools.read().unwrap();
        for name in names {
            if exclude_subagent_spawner && name == SPAWN_SUBAGENT_TOOL_NAME {
                continue;
            }
            if let Some((tool, opts)) = tools.get(name) {
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

    /// Return a new registry containing exactly the registered tools named in `names`.
    /// An empty list returns an empty registry.
    pub fn restricted_to_names(&self, names: &[String]) -> Arc<Self> {
        self.copy_named_tools(names, false)
    }

    /// Return a new registry containing only tools whose names appear in `names`.
    /// `spawn_subagent` is always excluded regardless of `names` — SubAgents cannot
    /// spawn sub-SubAgents.
    pub fn filter_by_names(&self, names: &[String]) -> Arc<ToolRegistry> {
        self.copy_named_tools(names, true)
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

    #[test]
    fn tool_definitions_for_ignores_unknown_names_and_preserves_order() {
        use crate::calculator::Calculator;
        use crate::datetime::DateTimeTool;

        let reg = ToolRegistry::new();
        reg.register(Calculator);
        reg.register(DateTimeTool);

        let names = vec![
            "datetime".to_string(),
            "ghost".to_string(),
            "calculator".to_string(),
        ];
        let definitions = reg.tool_definitions_for(&names).unwrap();

        assert_eq!(
            definitions
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            ["datetime", "calculator"]
        );
    }

    #[test]
    fn tool_definitions_for_maps_schema_fields() {
        use crate::calculator::Calculator;

        let reg = ToolRegistry::new();
        reg.register(Calculator);

        let definitions = reg
            .tool_definitions_for(&["calculator".to_string()])
            .unwrap();
        let definition = definitions.first().unwrap();
        let schema = reg
            .schema()
            .into_iter()
            .find(|schema| schema["name"] == "calculator")
            .unwrap();

        assert_eq!(definition.name, schema["name"].as_str().unwrap());
        assert_eq!(
            definition.description,
            schema["description"].as_str().unwrap()
        );
        // parameters is now the strict-adapted schema, not the raw one.
        assert_eq!(
            definition.parameters,
            crate::strict_schema::to_strict_schema(schema["input_schema"].clone()).unwrap()
        );
    }

    #[test]
    fn tool_definitions_for_returns_strict_schemas() {
        use crate::calculator::Calculator;

        let reg = ToolRegistry::new();
        reg.register(Calculator);

        let definitions = reg
            .tool_definitions_for(&["calculator".to_string()])
            .unwrap();
        let definition = definitions.first().unwrap();
        assert_eq!(
            definition.parameters["additionalProperties"],
            serde_json::Value::Bool(false)
        );
    }

    #[test]
    fn tool_definitions_for_propagates_strict_schema_errors() {
        use agentverse::{Tool, ToolResult};
        use schemars::JsonSchema;
        use serde::Deserialize;
        use std::collections::HashMap;

        #[allow(dead_code)]
        #[derive(Deserialize, JsonSchema)]
        struct OpenDictArgs {
            tags: HashMap<String, String>,
        }

        struct OpenDictTool;

        #[async_trait::async_trait]
        impl Tool for OpenDictTool {
            type Args = OpenDictArgs;
            fn name(&self) -> &str {
                "open_dict_tool"
            }
            fn description(&self) -> &str {
                "test tool with an open dictionary arg"
            }
            async fn execute(&self, _args: OpenDictArgs) -> ToolResult {
                Ok(serde_json::json!({}))
            }
        }

        let reg = ToolRegistry::new();
        reg.register(OpenDictTool);
        let result = reg.tool_definitions_for(&["open_dict_tool".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn filter_by_names_returns_only_named_tools() {
        use crate::calculator::Calculator;
        let reg = ToolRegistry::new();
        reg.register(Calculator);
        let filtered = reg.filter_by_names(&["calculator".to_string()]);
        assert!(filtered.has_tool("calculator"));
        assert!(!filtered.has_tool("find_tools"));
    }

    #[test]
    fn filter_by_names_never_includes_spawn_subagent() {
        let reg = ToolRegistry::new();
        // Even if explicitly listed, spawn_subagent is excluded
        let filtered =
            reg.filter_by_names(&["spawn_subagent".to_string(), "find_tools".to_string()]);
        assert!(!filtered.has_tool("spawn_subagent"));
    }

    #[test]
    fn filter_by_names_empty_list_returns_empty_registry() {
        let reg = ToolRegistry::new();
        let filtered = reg.filter_by_names(&[]);
        assert_eq!(filtered.len(), 0);
    }
}
