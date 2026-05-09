use agentverse::model::{ModelProvider, OpenAICompatible};
use httpmock::prelude::*;

#[tokio::test]
async fn test_openai_compatible_generate() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method("POST")
            .path("/chat/completions")
            .header("Authorization", "Bearer test-key")
            .json_body(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "hello"}]
            }));
        then.status(200).json_body(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Hello! How can I help you?"
                }
            }]
        }));
    });

    let model = OpenAICompatible::new(&server.base_url(), "test-model", "test-key");

    let result = model.generate("hello", None).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Hello! How can I help you?");

    mock.assert();
}
