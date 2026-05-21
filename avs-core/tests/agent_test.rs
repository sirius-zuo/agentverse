use agentverse::{Agent, Config, ProviderConfig};

fn make_config() -> Config {
    Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: "sk-xxx".to_string(),
            // Port 1 is always closed — forces a connection error without network traffic.
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
        },
        max_messages: 50,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    }
}

#[test]
fn test_agent_from_config_valid() {
    let config = make_config();
    let agent = Agent::from_config(config);
    assert!(agent.is_ok());
}

#[tokio::test]
async fn test_agent_invoke_calls_provider() {
    let config = make_config();
    let agent = Agent::from_config(config).unwrap();
    // Invalid credentials — real provider call fails, confirming invoke() no longer echoes.
    let result = agent.invoke("user1", "hello").await;
    assert!(result.is_err());
}
