use agentverse::{Tool, ToolError, ToolResult};
use glob::glob;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
pub struct FileSearchArgs {
    /// Directory to search in
    pub path: String,
    /// Glob pattern (e.g. "*.txt", "**/*.rs")
    pub pattern: String,
}

pub struct FileSearch;

#[async_trait::async_trait]
impl Tool for FileSearch {
    type Args = FileSearchArgs;

    fn name(&self) -> &str {
        "file_search"
    }

    fn description(&self) -> &str {
        "Search for files matching a pattern in a directory"
    }

    async fn execute(&self, args: FileSearchArgs) -> ToolResult {
        let full_pattern = format!("{}/{}", args.path, args.pattern);
        let matches: Vec<String> = glob(&full_pattern)
            .map_err(|e| ToolError::Execution(e.to_string()))?
            .filter_map(|entry| entry.ok())
            .filter_map(|p| p.to_str().map(String::from))
            .collect();

        Ok(json!({
            "matches": matches,
            "count": matches.len()
        }))
    }
}
