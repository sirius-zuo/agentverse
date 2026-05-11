use agentverse::{Agent, Config, ProviderConfig};

fn make_config() -> Config {
    Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: "sk-xxx".to_string(),
            base_url: None,
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
async fn test_agent_invoke_placeholder() {
    let config = make_config();
    let agent = Agent::from_config(config).unwrap();
    let result = agent.invoke("user1", "hello").await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("hello"));
}
