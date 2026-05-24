use agentverse::{Config, LlmRunner, LlmRunnerBuilder, ProviderConfig};
use agentverse::memory::{Message, MessageRole};

fn closed_port_config() -> Config {
    Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4o".to_string(),
            api_key: "sk-xxx".to_string(),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
        },
        max_messages: 50,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    }
}

#[test]
fn llm_runner_from_config_builds_successfully() {
    let config = closed_port_config();
    assert!(LlmRunner::from_config(config).is_ok());
}

#[tokio::test]
async fn llm_runner_invoke_takes_messages_and_returns_error_on_bad_port() {
    let runner = LlmRunner::from_config(closed_port_config()).unwrap();
    let messages = vec![Message { role: MessageRole::User, content: "hello".to_string() }];
    let result = runner.invoke(messages).await;
    assert!(result.is_err(), "expected network error on closed port");
}

#[test]
fn llm_runner_builder_sets_system_prompt() {
    let result = LlmRunnerBuilder::new()
        .config(closed_port_config())
        .system_prompt("You are a test assistant.")
        .build();
    assert!(result.is_ok());
}
