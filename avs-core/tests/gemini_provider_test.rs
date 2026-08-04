use agentverse::memory::{Message, MessageRole};
use agentverse::{GeminiProvider, GenerateRequest, ModelError, ModelProvider, ToolDefinition};
use serde_json::json;

// ── GeminiProvider tests ──────────────────────────────────────────────────────

#[test]
fn test_gemini_provider_name() {
    let provider = GeminiProvider::new();
    assert_eq!(provider.name(), "gemini");
}

#[test]
fn test_gemini_endpoint_path_includes_model() {
    let provider = GeminiProvider::new();
    assert_eq!(
        provider.endpoint_path("gemini-1.5-pro"),
        "/v1beta/models/gemini-1.5-pro:generateContent"
    );
}

#[test]
fn test_gemini_request_headers_is_empty() {
    let provider = GeminiProvider::new();
    let headers = provider.request_headers("my-api-key");
    // Gemini uses query param auth, not headers
    assert!(headers.is_empty());
}

#[test]
fn test_gemini_build_request_system_instruction() {
    let provider = GeminiProvider::new();
    let req = GenerateRequest {
        system: Some("System instruction".to_string()),
        messages: vec![Message::text(MessageRole::User, "hello")],
        tools: None,
        ..Default::default()
    };
    let body = provider.build_request("gemini-pro", req).unwrap();
    assert_eq!(
        body["system_instruction"]["parts"][0]["text"],
        "System instruction"
    );
}

#[test]
fn test_gemini_build_request_system_role_messages_filtered() {
    let provider = GeminiProvider::new();
    let req = GenerateRequest {
        system: Some("System instruction".to_string()),
        messages: vec![
            Message::text(MessageRole::System, "this should be filtered"),
            Message::text(MessageRole::User, "hello"),
        ],
        tools: None,
        ..Default::default()
    };
    let body = provider.build_request("gemini-pro", req).unwrap();
    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["role"], "user");
}

#[test]
fn test_gemini_build_request_with_tools_is_error() {
    // GeminiProvider does not support native tool-calling: any request that
    // offers tool schemas must hard-error rather than silently mapping them
    // into a `functions` field the Gemini API doesn't use for tool calling.
    let provider = GeminiProvider::new();
    let req = GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hello")],
        tools: Some(vec![ToolDefinition {
            name: "calc".to_string(),
            description: "Calculate".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        }]),
        ..Default::default()
    };
    let err = provider.build_request("gemini-pro", req).unwrap_err();
    assert!(matches!(err, ModelError::InvalidResponse(_)));
}

#[test]
fn test_gemini_parse_response_ok() {
    let provider = GeminiProvider::new();
    let body = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "Hello from Gemini!"}]
            }
        }]
    })
    .to_string();
    let result = provider.parse_response(&body).unwrap();
    assert_eq!(result.as_text(), "Hello from Gemini!");
}

#[test]
fn test_gemini_parse_response_empty_candidates_is_error() {
    let provider = GeminiProvider::new();
    let body = json!({"candidates": []}).to_string();
    let err = provider.parse_response(&body).unwrap_err();
    assert!(err.to_string().contains("No content"));
}
