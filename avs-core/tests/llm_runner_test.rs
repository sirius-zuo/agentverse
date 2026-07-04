use agentverse::memory::{Message, MessageRole};
use agentverse::{Config, LlmRunner, LlmRunnerBuilder, ProviderConfig};

fn closed_port_config() -> Config {
    Config {
        provider: ProviderConfig::openai(
            "gpt-4o".to_string(),
            "sk-xxx".to_string(),
            Some("http://127.0.0.1:1/v1".to_string()),
        ),
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
    let messages = vec![Message {
        role: MessageRole::User,
        content: "hello".to_string(),
    }];
    let result = runner.invoke(messages).await;
    assert!(result.is_err(), "expected network error on closed port");
}

#[tokio::test]
async fn llm_runner_invoke_with_system_message_fails_on_bad_port() {
    let runner = LlmRunner::from_config(closed_port_config()).unwrap();
    let messages = vec![
        Message {
            role: MessageRole::System,
            content: "You are a helpful assistant.".to_string(),
        },
        Message {
            role: MessageRole::User,
            content: "hello".to_string(),
        },
    ];
    let result = runner.invoke(messages).await;
    assert!(result.is_err());
}

#[test]
fn llm_runner_builder_builds_from_config() {
    let result = LlmRunnerBuilder::new().config(closed_port_config()).build();
    assert!(result.is_ok());
}
