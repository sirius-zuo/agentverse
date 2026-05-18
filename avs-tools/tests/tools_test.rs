use agentverse::SyncTool;
use agentverse_tools::{Calculator, DateTimeTool, FileSearch, HttpClient, SyncToolAdapter, ToolRegistry};
use serde_json::json;

#[tokio::test]
async fn test_registry_register_and_execute() {
    let mut registry = ToolRegistry::new();
    registry.register(SyncToolAdapter(Calculator));
    assert!(registry.has_tool("calculator"));
    let result = registry
        .execute("calculator", json!({ "operation": "add", "a": 2.0, "b": 3.0 }))
        .await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["result"], 5.0);
}

#[tokio::test]
async fn test_registry_not_found() {
    let registry = ToolRegistry::new();
    let result = registry.execute("nonexistent", json!({})).await;
    assert!(result.is_err());
}

#[test]
fn test_registry_tool_names() {
    let mut registry = ToolRegistry::new();
    registry.register(SyncToolAdapter(Calculator));
    registry.register(SyncToolAdapter(DateTimeTool));
    let names = registry.tool_names();
    assert!(names.contains(&"calculator".to_string()));
    assert!(names.contains(&"datetime".to_string()));
}

#[test]
fn test_calculator_add() {
    let tool = Calculator;
    let result = tool.execute(json!({
        "operation": "add",
        "a": 10.0,
        "b": 5.0
    }));
    assert_eq!(result.unwrap()["result"], 15.0);
}

#[test]
fn test_calculator_subtract() {
    let tool = Calculator;
    let result = tool.execute(json!({
        "operation": "subtract",
        "a": 10.0,
        "b": 3.0
    }));
    assert_eq!(result.unwrap()["result"], 7.0);
}

#[test]
fn test_calculator_multiply() {
    let tool = Calculator;
    let result = tool.execute(json!({
        "operation": "multiply",
        "a": 4.0,
        "b": 3.0
    }));
    assert_eq!(result.unwrap()["result"], 12.0);
}

#[test]
fn test_calculator_divide() {
    let tool = Calculator;
    let result = tool.execute(json!({
        "operation": "divide",
        "a": 10.0,
        "b": 2.0
    }));
    assert_eq!(result.unwrap()["result"], 5.0);
}

#[test]
fn test_calculator_division_by_zero() {
    let tool = Calculator;
    let result = tool.execute(json!({
        "operation": "divide",
        "a": 10.0,
        "b": 0.0
    }));
    assert!(result.is_err());
}

#[test]
fn test_calculator_unknown_operation() {
    let tool = Calculator;
    let result = tool.execute(json!({
        "operation": "mod",
        "a": 10.0,
        "b": 3.0
    }));
    assert!(result.is_err());
}

#[test]
fn test_calculator_missing_params() {
    let tool = Calculator;
    let result = tool.execute(json!({
        "operation": "add",
        "a": 1.0
    }));
    assert!(result.is_err());
}

#[test]
fn test_datetime() {
    let tool = DateTimeTool;
    let result = tool.execute(json!({}));
    let val = result.unwrap();
    assert!(val["utc"].is_string());
    assert!(val["unix_timestamp"].is_i64());
    assert!(val["date"].is_string());
    assert!(val["time"].is_string());
}

#[test]
fn test_file_search() {
    let tool = FileSearch;
    // Search in current directory for any Cargo.toml
    let result = tool.execute(json!({
        "path": ".",
        "pattern": "**/Cargo.toml"
    }));
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val["matches"].is_array());
    assert!(val["count"].is_number());
}

#[test]
fn test_file_search_missing_params() {
    let tool = FileSearch;
    let result = tool.execute(json!({
        "pattern": "*.txt"
    }));
    assert!(result.is_err());
}

#[test]
fn test_http_client_parameters() {
    use agentverse::AsyncTool;
    let tool = HttpClient;
    let params = tool.parameters();
    assert!(params["required"]
        .as_array()
        .unwrap()
        .contains(&json!("method")));
    assert!(params["required"]
        .as_array()
        .unwrap()
        .contains(&json!("url")));
}

#[tokio::test]
async fn test_http_client_missing_method() {
    use agentverse::AsyncTool;
    let tool = HttpClient;
    let result = tool.execute(json!({ "url": "http://example.com" })).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_http_client_unsupported_method() {
    use agentverse::AsyncTool;
    let tool = HttpClient;
    let result = tool
        .execute(json!({ "method": "PATCH", "url": "http://example.com" }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_http_client_file_scheme_blocked() {
    use agentverse::AsyncTool;
    let tool = HttpClient;
    let result = tool
        .execute(json!({ "method": "GET", "url": "file:///etc/passwd" }))
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("http and https"));
}

#[tokio::test]
async fn test_http_client_invalid_url() {
    use agentverse::AsyncTool;
    let tool = HttpClient;
    let result = tool
        .execute(json!({ "method": "GET", "url": "not a valid url" }))
        .await;
    assert!(result.is_err());
}

#[test]
fn test_registry_register_many() {
    // register_many removed; use register() per tool
    let mut registry = ToolRegistry::new();
    registry.register(SyncToolAdapter(Calculator));
    registry.register(SyncToolAdapter(DateTimeTool));
    assert_eq!(registry.len(), 2);
}

#[test]
fn test_registry_filter_category() {
    let mut registry = ToolRegistry::new();
    registry.register_with_category(SyncToolAdapter(Calculator), "math");
    registry.register_with_category(SyncToolAdapter(DateTimeTool), "utility");

    let math = registry.filter_category("math");
    assert!(math.has_tool("calculator"));
    assert!(!math.has_tool("datetime"));

    let util = registry.filter_category("utility");
    assert!(!util.has_tool("calculator"));
    assert!(util.has_tool("datetime"));
}

#[test]
fn test_registry_schema() {
    let mut registry = ToolRegistry::new();
    registry.register(SyncToolAdapter(Calculator));
    let schema = registry.schema();
    assert_eq!(schema.len(), 1);
    assert_eq!(schema[0]["name"], "calculator");
    assert!(schema[0]["description"].is_string());
    assert!(schema[0]["parameters"].is_object());
}

#[test]
fn test_registry_len_and_empty() {
    let mut registry = ToolRegistry::new();
    assert!(registry.is_empty());
    registry.register(SyncToolAdapter(Calculator));
    assert_eq!(registry.len(), 1);
    assert!(!registry.is_empty());
}

// ─── Additional FileSearch tests ─────────────────────────────────────────────

#[test]
fn test_file_search_no_matches() {
    let tool = FileSearch;
    let result = tool.execute(json!({
        "path": ".",
        "pattern": "*.zzznonexistent_extension_xyz"
    }));
    assert!(result.is_ok());
    let val = result.unwrap();
    assert_eq!(val["count"], 0);
    assert_eq!(val["matches"].as_array().unwrap().len(), 0);
}

#[test]
fn test_file_search_rs_files() {
    let tool = FileSearch;
    let src_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let result = tool.execute(json!({
        "path": src_dir,
        "pattern": "*.rs"
    }));
    assert!(result.is_ok());
    let val = result.unwrap();
    let count = val["count"].as_u64().unwrap_or(0);
    assert!(count > 0, "Expected .rs files in avs-tools/src");
}

#[test]
fn test_file_search_pattern_missing_param() {
    let tool = FileSearch;
    let result = tool.execute(json!({ "path": "." }));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("pattern"));
}

#[test]
fn test_file_search_tool_metadata() {
    let tool = FileSearch;
    assert_eq!(tool.name(), "file_search");
    let params = tool.parameters();
    let required = params["required"].as_array().unwrap();
    assert!(required.contains(&json!("path")));
    assert!(required.contains(&json!("pattern")));
}

// ─── SyncToolAdapter tests ────────────────────────────────────────────────────

#[tokio::test]
async fn test_sync_tool_adapter_metadata() {
    use agentverse::AsyncTool;
    use agentverse_tools::SyncToolAdapter;
    let adapter = SyncToolAdapter(Calculator);
    assert_eq!(adapter.name(), "calculator");
    assert!(!adapter.description().is_empty());
    let params = adapter.parameters();
    assert!(params["required"].is_array());
}

#[tokio::test]
async fn test_sync_tool_adapter_execute_delegates() {
    use agentverse::AsyncTool;
    use agentverse_tools::SyncToolAdapter;
    let adapter = SyncToolAdapter(Calculator);
    let result = adapter
        .execute(json!({ "operation": "add", "a": 2.0, "b": 3.0 }))
        .await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["result"], 5.0);
}

#[tokio::test]
async fn test_sync_tool_adapter_propagates_error() {
    use agentverse::AsyncTool;
    use agentverse_tools::SyncToolAdapter;
    let adapter = SyncToolAdapter(Calculator);
    let result = adapter
        .execute(json!({ "operation": "divide", "a": 1.0, "b": 0.0 }))
        .await;
    assert!(result.is_err());
}

// ─── ShellTool tests ─────────────────────────────────────────────────────────

use agentverse_tools::ShellTool;
use std::time::Duration;

#[tokio::test]
async fn test_shell_basic_stdout() {
    use agentverse::AsyncTool;
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool.execute(json!({ "command": "echo hello" })).await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert_eq!(val["stdout"].as_str().unwrap().trim(), "hello");
    assert_eq!(val["exit_code"], 0);
}

#[tokio::test]
async fn test_shell_nonzero_exit_is_not_an_error() {
    use agentverse::AsyncTool;
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    // "false" is a POSIX command that always exits with code 1
    let result = tool.execute(json!({ "command": "false" })).await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert_ne!(val["exit_code"], 0);
}

#[tokio::test]
async fn test_shell_stderr_captured() {
    use agentverse::AsyncTool;
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool
        .execute(json!({ "command": "ls /this_path_does_not_exist_xyz" }))
        .await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(!val["stderr"].as_str().unwrap_or("").is_empty());
}

#[tokio::test]
async fn test_shell_blocked_command() {
    use agentverse::AsyncTool;
    // Block "echo" — a command that would otherwise succeed — so the test only
    // passes if the actual block logic fires, not because the binary is absent.
    let tool = ShellTool::new(".", Duration::from_secs(5), vec!["echo".to_string()]);
    let result = tool.execute(json!({ "command": "echo hi" })).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("blocked"));
}

#[tokio::test]
async fn test_shell_timeout() {
    use agentverse::AsyncTool;
    let tool = ShellTool::new(
        ".",
        Duration::from_millis(100),
        Vec::<String>::new(),
    );
    let result = tool.execute(json!({ "command": "sleep 10" })).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("timed out") || msg.contains("timeout"),
        "expected timeout message, got: {msg}"
    );
}

#[tokio::test]
async fn test_shell_missing_command_param() {
    use agentverse::AsyncTool;
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool.execute(json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_shell_empty_command() {
    use agentverse::AsyncTool;
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool.execute(json!({ "command": "" })).await;
    assert!(result.is_err());
}

#[test]
fn test_shell_metadata() {
    use agentverse::AsyncTool;
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    assert_eq!(tool.name(), "shell");
    let params = tool.parameters();
    assert!(params["required"]
        .as_array()
        .unwrap()
        .contains(&json!("command")));
}
