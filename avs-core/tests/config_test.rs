use agentverse::{Config, ProviderConfig};

fn make_config() -> Config {
    Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: "sk-xxx".to_string(),
            base_url: None,
        },
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    }
}

#[test]
fn test_config_validation_missing_key() {
    let config = Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: String::new(),
            base_url: None,
        },
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_missing_name() {
    let config = Config {
        provider: ProviderConfig::OpenAI {
            model_name: String::new(),
            api_key: "sk-xxx".to_string(),
            base_url: None,
        },
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_valid() {
    let config = make_config();
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_serialization() {
    let config = Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: "sk-xxx".to_string(),
            base_url: Some("http://localhost:9090/v1".to_string()),
        },
        max_messages: 200,
        tools: vec!["search".to_string()],
        prompts_dir: Some("prompts/".to_string()),
        system_prompt: Some("You are helpful.".to_string()),
    };
    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("model_name: gpt-4"));
    let deserialized: Config = serde_yaml::from_str(&yaml).unwrap();
    assert!(matches!(
        deserialized.provider,
        ProviderConfig::OpenAI { .. }
    ));
    assert_eq!(deserialized.prompts_dir, Some("prompts/".to_string()));
    assert_eq!(
        deserialized.system_prompt,
        Some("You are helpful.".to_string())
    );
}

#[test]
fn test_provider_config_anthropic() {
    let config = Config {
        provider: ProviderConfig::Anthropic {
            model_name: "claude-3".to_string(),
            api_key: "anthropic-key".to_string(),
        },
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
        provider: ProviderConfig::Gemini {
            model_name: "gemini-pro".to_string(),
            api_key: "gemini-key".to_string(),
        },
        max_messages: 50,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };
    assert!(config.validate().is_ok());
}
