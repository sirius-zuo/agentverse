use agentverse::SyncTool;
use agentverse_tools::{Calculator, DateTimeTool, FileSearch, HttpClient, ToolRegistry};
use serde_json::json;

#[test]
fn test_registry_register_and_execute() {
    let mut registry = ToolRegistry::new();
    registry.register(Calculator);

    assert!(registry.has_tool("calculator"));
    let result = registry.execute("calculator", json!({
        "operation": "add",
        "a": 2.0,
        "b": 3.0
    }));
    assert!(result.is_ok());
    let val = result.unwrap();
    assert_eq!(val["result"], 5.0);
}

#[test]
fn test_registry_not_found() {
    let registry = ToolRegistry::new();
    let result = registry.execute("nonexistent", json!({}));
    assert!(result.is_err());
}

#[test]
fn test_registry_tool_names() {
    let mut registry = ToolRegistry::new();
    registry.register(Calculator);
    registry.register(DateTimeTool);

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
    let tool = HttpClient;
    let params = tool.parameters();
    assert!(params["required"].as_array().unwrap().contains(&json!("method")));
    assert!(params["required"].as_array().unwrap().contains(&json!("url")));
}

#[test]
fn test_http_client_missing_method() {
    let tool = HttpClient;
    let result = tool.execute(json!({
        "url": "http://example.com"
    }));
    assert!(result.is_err());
}

#[test]
fn test_http_client_unsupported_method() {
    let tool = HttpClient;
    let result = tool.execute(json!({
        "method": "PATCH",
        "url": "http://example.com"
    }));
    assert!(result.is_err());
}

#[test]
fn test_http_client_file_scheme_blocked() {
    let tool = HttpClient;
    let result = tool.execute(json!({
        "method": "GET",
        "url": "file:///etc/passwd"
    }));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("http and https"));
}

#[test]
fn test_http_client_invalid_url() {
    let tool = HttpClient;
    let result = tool.execute(json!({
        "method": "GET",
        "url": "not a valid url"
    }));
    assert!(result.is_err());
}

#[test]
fn test_registry_register_many() {
    let mut registry = ToolRegistry::new();
    registry.register_many(vec![
        Box::new(Calculator),
        Box::new(DateTimeTool),
    ]);
    assert_eq!(registry.tool_names().len(), 2);
}
