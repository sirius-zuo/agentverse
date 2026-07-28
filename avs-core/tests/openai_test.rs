use agentverse::memory::{Message, MessageRole};
use agentverse::model::{GenerateRequest, ModelProvider, OpenAICompatible};

#[test]
fn test_openai_compatible_build_request() {
    let model = OpenAICompatible::new();

    let body = model
        .build_request(
            "test-model",
            GenerateRequest {
                system: None,
                messages: vec![Message::text(MessageRole::User, "hello")],
                tools: None,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(body["model"], "test-model");
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "hello");
    // disable_thinking is on by default
    assert_eq!(
        body["chat_template_kwargs"]["enable_thinking"],
        serde_json::Value::Bool(false)
    );
}

#[test]
fn test_openai_compatible_request_headers() {
    let model = OpenAICompatible::new();
    let headers = model.request_headers("test-key");
    assert!(headers.contains_key("authorization"));
    let auth = headers["authorization"].to_str().unwrap();
    assert!(auth.contains("Bearer"));
    assert!(auth.contains("test-key"));
}

#[test]
fn test_openai_compatible_endpoint_path() {
    let model = OpenAICompatible::new();
    assert_eq!(model.endpoint_path("any-model"), "/chat/completions");
}

#[test]
fn test_openai_compatible_parse_response() {
    let model = OpenAICompatible::new();
    let body = r#"{
        "choices": [{"message": {"content": "Hello! How can I help you?"}}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 8}
    }"#;
    let result = model.parse_response(body).unwrap();
    assert_eq!(result.as_text(), "Hello! How can I help you?");
    assert_eq!(result.usage.input_tokens, 5);
    assert_eq!(result.usage.output_tokens, 8);
}

#[test]
fn response_format_serialized_as_json_schema_wrapper() {
    let model = OpenAICompatible::new();
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "nodes": { "type": "array" } }
    });
    let body = model
        .build_request(
            "qwen",
            GenerateRequest {
                system: None,
                messages: vec![Message::text(MessageRole::User, "plan this")],
                tools: None,
                response_format: Some(schema.clone()),
            },
        )
        .unwrap();
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert_eq!(body["response_format"]["json_schema"]["name"], "response");
    assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
}

#[test]
fn response_format_omitted_when_none() {
    let model = OpenAICompatible::new();
    let body = model
        .build_request(
            "qwen",
            GenerateRequest {
                system: None,
                messages: vec![Message::text(MessageRole::User, "hello")],
                tools: None,
                response_format: None,
            },
        )
        .unwrap();
    assert!(body.get("response_format").is_none());
}
