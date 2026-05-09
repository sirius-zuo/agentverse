use agentverse::{Agent, Config};

#[test]
fn test_agent_from_config_valid() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
    };
    let agent = Agent::from_config(config);
    assert!(agent.is_ok());
}

#[tokio::test]
async fn test_agent_invoke_placeholder() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
    };
    let agent = Agent::from_config(config).unwrap();
    let result = agent.invoke("user1", "hello").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Processed: hello");
}
