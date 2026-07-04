use agentverse::ErasedTool;
use agentverse_tools::{
    Calculator, DateTimeTool, FileSearch, HttpClient, ToolOptions, ToolRegistry,
};
use serde_json::json;

// ─── Registry tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_registry_register_and_execute() {
    let registry = ToolRegistry::new();
    registry.register(Calculator);
    assert!(registry.has_tool("calculator"));
    let result = registry
        .execute(
            "calculator",
            json!({ "operation": "add", "a": 2.0, "b": 3.0 }),
        )
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
    let registry = ToolRegistry::new();
    registry.register(Calculator);
    registry.register(DateTimeTool);
    let names = registry.tool_names();
    assert!(names.contains(&"calculator".to_string()));
    assert!(names.contains(&"datetime".to_string()));
}

// ─── Calculator tests (via ErasedTool::execute_raw) ───────────────────────────

#[tokio::test]
async fn test_calculator_add() {
    let result = Calculator
        .execute_raw(json!({ "operation": "add", "a": 10.0, "b": 5.0 }))
        .await;
    assert_eq!(result.unwrap()["result"], 15.0);
}

#[tokio::test]
async fn test_calculator_subtract() {
    let result = Calculator
        .execute_raw(json!({ "operation": "subtract", "a": 10.0, "b": 3.0 }))
        .await;
    assert_eq!(result.unwrap()["result"], 7.0);
}

#[tokio::test]
async fn test_calculator_multiply() {
    let result = Calculator
        .execute_raw(json!({ "operation": "multiply", "a": 4.0, "b": 3.0 }))
        .await;
    assert_eq!(result.unwrap()["result"], 12.0);
}

#[tokio::test]
async fn test_calculator_divide() {
    let result = Calculator
        .execute_raw(json!({ "operation": "divide", "a": 10.0, "b": 2.0 }))
        .await;
    assert_eq!(result.unwrap()["result"], 5.0);
}

#[tokio::test]
async fn test_calculator_division_by_zero() {
    let result = Calculator
        .execute_raw(json!({ "operation": "divide", "a": 10.0, "b": 0.0 }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_calculator_unknown_operation() {
    // "mod" is not a valid Operation variant — serde deserialization fails
    let result = Calculator
        .execute_raw(json!({ "operation": "mod", "a": 10.0, "b": 3.0 }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_calculator_missing_params() {
    // Missing "b" — serde deserialization fails with InvalidArgs
    let result = Calculator
        .execute_raw(json!({ "operation": "add", "a": 1.0 }))
        .await;
    assert!(result.is_err());
}

// ─── DateTime tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_datetime() {
    let val = DateTimeTool.execute_raw(json!({})).await.unwrap();
    assert!(val["utc"].is_string());
    assert!(val["unix_timestamp"].is_i64());
    assert!(val["date"].is_string());
    assert!(val["time"].is_string());
}

// ─── FileSearch tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_file_search() {
    let result = FileSearch
        .execute_raw(json!({ "path": ".", "pattern": "**/Cargo.toml" }))
        .await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val["matches"].is_array());
    assert!(val["count"].is_number());
}

#[tokio::test]
async fn test_file_search_missing_params() {
    // Missing "path" field — deserialization fails
    let result = FileSearch.execute_raw(json!({ "pattern": "*.txt" })).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_file_search_no_matches() {
    let result = FileSearch
        .execute_raw(json!({ "path": ".", "pattern": "*.zzznonexistent_extension_xyz" }))
        .await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert_eq!(val["count"], 0);
    assert_eq!(val["matches"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_file_search_rs_files() {
    let src_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let result = FileSearch
        .execute_raw(json!({ "path": src_dir, "pattern": "*.rs" }))
        .await;
    assert!(result.is_ok());
    let val = result.unwrap();
    let count = val["count"].as_u64().unwrap_or(0);
    assert!(count > 0, "Expected .rs files in avs-tools/src");
}

#[tokio::test]
async fn test_file_search_pattern_missing_param() {
    // Missing "pattern" field
    let result = FileSearch.execute_raw(json!({ "path": "." })).await;
    assert!(result.is_err());
}

#[test]
fn test_file_search_tool_metadata() {
    assert_eq!(FileSearch.name(), "file_search");
    let schema = FileSearch.schema();
    let required = schema["input_schema"]["required"].as_array().unwrap();
    assert!(required.contains(&json!("path")));
    assert!(required.contains(&json!("pattern")));
}

// ─── HttpClient tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_http_client_missing_method() {
    // Missing required "method" field
    let result = HttpClient
        .execute_raw(json!({ "url": "http://example.com" }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_http_client_unsupported_method() {
    let result = HttpClient
        .execute_raw(json!({ "method": "PATCH", "url": "http://example.com" }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_http_client_file_scheme_blocked() {
    let result = HttpClient
        .execute_raw(json!({ "method": "GET", "url": "file:///etc/passwd" }))
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("http and https"));
}

#[tokio::test]
async fn test_http_client_invalid_url() {
    let result = HttpClient
        .execute_raw(json!({ "method": "GET", "url": "not a valid url" }))
        .await;
    assert!(result.is_err());
}

// ─── Registry API tests ───────────────────────────────────────────────────────

#[test]
fn test_registry_register_many() {
    let registry = ToolRegistry::new();
    registry.register(Calculator);
    registry.register(DateTimeTool);
    // find_tools is also auto-registered
    assert!(registry.len() >= 2);
    assert!(registry.has_tool("calculator"));
    assert!(registry.has_tool("datetime"));
}

#[test]
fn test_registry_filter_category() {
    let registry = ToolRegistry::new();
    registry.register_with_options(
        Calculator,
        ToolOptions {
            category: Some("math".into()),
            ..Default::default()
        },
    );
    registry.register_with_options(
        DateTimeTool,
        ToolOptions {
            category: Some("utility".into()),
            ..Default::default()
        },
    );

    let math = registry.filter_category("math");
    assert!(math.has_tool("calculator"));
    assert!(!math.has_tool("datetime"));

    let util = registry.filter_category("utility");
    assert!(!util.has_tool("calculator"));
    assert!(util.has_tool("datetime"));
}

#[test]
fn test_registry_schema() {
    let registry = ToolRegistry::new();
    registry.register(Calculator);
    let schema = registry.schema();
    let calc_schema = schema.iter().find(|s| s["name"] == "calculator").unwrap();
    assert!(calc_schema["description"].is_string());
    assert!(calc_schema["input_schema"].is_object());
}

#[test]
fn test_registry_len_and_empty() {
    let registry = ToolRegistry::new();
    // find_tools is auto-registered, so not empty
    assert!(!registry.is_empty());
    let initial_len = registry.len();
    registry.register(Calculator);
    assert_eq!(registry.len(), initial_len + 1);
}

// ─── ShellTool tests ──────────────────────────────────────────────────────────

use agentverse_tools::ShellTool;
use std::time::Duration;

#[tokio::test]
async fn test_shell_basic_stdout() {
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool.execute_raw(json!({ "command": "echo hello" })).await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert_eq!(val.as_str().unwrap().trim(), "hello");
}

#[tokio::test]
async fn test_shell_nonzero_exit_is_not_an_error() {
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool.execute_raw(json!({ "command": "false" })).await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.as_str().unwrap().contains("[exit code:"));
}

#[tokio::test]
async fn test_shell_stderr_captured() {
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool
        .execute_raw(json!({ "command": "ls /this_path_does_not_exist_xyz" }))
        .await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.as_str().unwrap().contains("[stderr:"));
}

#[tokio::test]
async fn test_shell_blocked_command() {
    let tool = ShellTool::new(".", Duration::from_secs(5), vec!["echo".to_string()]);
    let result = tool.execute_raw(json!({ "command": "echo hi" })).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("blocked"));
}

#[tokio::test]
async fn test_shell_timeout() {
    let tool = ShellTool::new(".", Duration::from_millis(100), Vec::<String>::new());
    let result = tool.execute_raw(json!({ "command": "sleep 10" })).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("timed out") || msg.contains("timeout"),
        "expected timeout message, got: {msg}"
    );
}

#[tokio::test]
async fn test_shell_missing_command_param() {
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool.execute_raw(json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_shell_empty_command() {
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    let result = tool.execute_raw(json!({ "command": "" })).await;
    assert!(result.is_err());
}

#[test]
fn test_shell_metadata() {
    let tool = ShellTool::new(".", Duration::from_secs(5), Vec::<String>::new());
    assert_eq!(tool.name(), "shell");
    let schema = tool.schema();
    let required = schema["input_schema"]["required"].as_array().unwrap();
    assert!(required.contains(&json!("command")));
}

// ─── WebSearch tests ──────────────────────────────────────────────────────────

use agentverse_tools::WebSearch;

const SAMPLE_DDG_HTML: &str = r#"
    <div class="result">
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Frust-lang.org&rut=abc">The Rust Programming Language</a>
        <span class="result__snippet">A systems programming language focused on safety.</span>
    </div>
    <div class="result">
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust-lang.org&rut=def">Rust Documentation</a>
        <span class="result__snippet">Official Rust language documentation.</span>
    </div>
    <div class="result">
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fcrates.io&rut=ghi">crates.io</a>
        <span class="result__snippet">Rust package registry.</span>
    </div>
"#;

#[test]
fn web_search_parse_extracts_all_results() {
    let results = agentverse_tools::web_search::parse_ddg_html(SAMPLE_DDG_HTML, 10);
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].0, "The Rust Programming Language");
    assert_eq!(results[0].1, "https://rust-lang.org");
    assert_eq!(
        results[0].2,
        "A systems programming language focused on safety."
    );
}

#[test]
fn web_search_parse_respects_max_results() {
    let results = agentverse_tools::web_search::parse_ddg_html(SAMPLE_DDG_HTML, 1);
    assert_eq!(results.len(), 1);
}

#[test]
fn web_search_parse_extracts_all_urls() {
    let results = agentverse_tools::web_search::parse_ddg_html(SAMPLE_DDG_HTML, 10);
    assert_eq!(results[1].1, "https://doc.rust-lang.org");
    assert_eq!(results[2].1, "https://crates.io");
}

#[test]
fn web_search_parse_direct_urls() {
    let html = r#"
        <div class="result">
            <a class="result__a" href="https://rust-lang.org/">The Rust Programming Language</a>
            <span class="result__snippet">A systems programming language.</span>
        </div>
        <div class="result">
            <a class="result__a" href="https://doc.rust-lang.org/book/">The Rust Book</a>
            <span class="result__snippet">The official Rust book.</span>
        </div>
    "#;
    let results = agentverse_tools::web_search::parse_ddg_html(html, 10);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].1, "https://rust-lang.org/");
    assert_eq!(results[1].1, "https://doc.rust-lang.org/book/");
}

#[test]
fn web_search_tool_name() {
    assert_eq!(WebSearch.name(), "web_search");
}

#[tokio::test]
#[ignore = "hits real network"]
async fn web_search_live_returns_results() {
    let result = WebSearch
        .execute_raw(json!({ "query": "rust programming language", "max_results": 2 }))
        .await;
    assert!(result.is_ok());
    let arr = result.unwrap();
    assert!(!arr.as_array().unwrap().is_empty());
    assert!(arr[0]["title"].as_str().is_some());
    assert!(arr[0]["url"].as_str().is_some());
}
