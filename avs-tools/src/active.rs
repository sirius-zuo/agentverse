use std::collections::HashSet;
use serde_json::Value;
use crate::registry::ToolRegistry;

/// Per-invocation set of tool names whose schemas are included in the LLM prompt.
/// All tools remain executable via ToolRegistry regardless of the active set.
#[derive(Default, Clone)]
pub struct ActiveToolSet {
    names: HashSet<String>,
}

impl ActiveToolSet {
    /// Build from all currently registered tool names (full set).
    pub fn all(registry: &ToolRegistry) -> Self {
        Self {
            names: registry.tool_names().into_iter().collect(),
        }
    }

    pub fn activate(&mut self, names: &[&str]) {
        for n in names {
            self.names.insert(n.to_string());
        }
    }

    pub fn deactivate(&mut self, names: &[&str]) {
        for n in names {
            self.names.remove(*n);
        }
    }

    /// Return schemas for only the active tools (unknown names silently skipped).
    pub fn schemas(&self, registry: &ToolRegistry) -> Vec<Value> {
        registry
            .schema()
            .into_iter()
            .filter(|s| {
                s["name"]
                    .as_str()
                    .map(|n| self.names.contains(n))
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}
