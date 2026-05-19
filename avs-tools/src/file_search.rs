use agentverse::{AsyncTool, ToolResult};
use async_trait::async_trait;
use glob::glob;
use serde_json::{json, Value};

/// Search files by pattern.
pub struct FileSearch;

#[async_trait]
impl AsyncTool for FileSearch {
    fn name(&self) -> &str {
        "file_search"
    }

    fn description(&self) -> &str {
        "Search for files matching a pattern in a directory"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to search in"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g., '*.txt', '**/*.rs')"
                }
            },
            "required": ["path", "pattern"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let path = args["path"].as_str().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing or invalid 'path' parameter".to_string())
        })?;

        let pattern = args["pattern"].as_str().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing or invalid 'pattern' parameter".to_string())
        })?;

        let full_pattern = format!("{}/{}", path, pattern);
        let matches: Vec<String> = glob(&full_pattern)
            .map_err(|e| agentverse::ToolError::Execution(e.to_string()))?
            .filter_map(|entry| entry.ok())
            .filter_map(|p| p.to_str().map(String::from))
            .collect();

        Ok(json!({
            "matches": matches,
            "count": matches.len()
        }))
    }
}
