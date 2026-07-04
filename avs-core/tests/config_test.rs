use agentverse::{Config, ConnectionManager, ProviderConfig, ProviderRegistry};

fn make_config() -> Config {
    Config {
        provider: ProviderConfig::openai("gpt-4".to_string(), "sk-xxx".to_string(), None),
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    }
}

#[test]
fn test_config_validation_valid() {
    let config = make_config();
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_missing_provider_name() {
    let config = Config {
        provider: ProviderConfig::custom("", std::collections::HashMap::new()),
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };
    assert!(config.validate().is_err());
}

#[test]
fn connection_manager_from_config_missing_api_key_errors() {
    let config = ProviderConfig::openai("gpt-4".to_string(), String::new(), None);
    let registry = ProviderRegistry::with_builtins();
    assert!(ConnectionManager::from_config(config, &registry).is_err());
}

#[test]
fn connection_manager_from_config_missing_model_name_errors() {
    let config = ProviderConfig::openai(String::new(), "sk-xxx".to_string(), None);
    let registry = ProviderRegistry::with_builtins();
    assert!(ConnectionManager::from_config(config, &registry).is_err());
}

#[test]
fn test_config_serialization() {
    let config = Config {
        provider: ProviderConfig::openai(
            "gpt-4".to_string(),
            "sk-xxx".to_string(),
            Some("http://localhost:9090/v1".to_string()),
        ),
        max_messages: 200,
        tools: vec!["search".to_string()],
        prompts_dir: Some("prompts/".to_string()),
        system_prompt: Some("You are helpful.".to_string()),
    };
    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("model_name: gpt-4"));
    let deserialized: Config = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(deserialized.provider.name, "openai");
    assert_eq!(deserialized.prompts_dir, Some("prompts/".to_string()));
    assert_eq!(
        deserialized.system_prompt,
        Some("You are helpful.".to_string())
    );
}

#[test]
fn test_provider_config_anthropic() {
    let config = Config {
        provider: ProviderConfig::anthropic("claude-3".to_string(), "anthropic-key".to_string()),
        max_messages: 50,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_provider_config_gemini() {
    let config = Config {
        provider: ProviderConfig::gemini("gemini-pro".to_string(), "gemini-key".to_string()),
        max_messages: 50,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };
    assert!(config.validate().is_ok());
}
