use agentverse::Config;

fn make_config() -> Config {
    Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    }
}

#[test]
fn test_config_validation_missing_key() {
    let config = Config {
        model_api_key: String::new(),
        model_name: "gpt-4".to_string(),
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
        model_api_key: "sk-xxx".to_string(),
        model_name: String::new(),
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
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 200,
        tools: vec!["search".to_string()],
        prompts_dir: Some("prompts/".to_string()),
        system_prompt: Some("You are helpful.".to_string()),
    };
    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("model_name: gpt-4"));
    let deserialized: Config = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(deserialized.model_name, "gpt-4");
    assert_eq!(deserialized.prompts_dir, Some("prompts/".to_string()));
    assert_eq!(deserialized.system_prompt, Some("You are helpful.".to_string()));
}
