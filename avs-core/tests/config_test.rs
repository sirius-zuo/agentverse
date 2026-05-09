use agentverse::Config;

#[test]
fn test_config_validation_missing_key() {
    let config = Config {
        model_api_key: String::new(),
        model_name: "gpt-4".to_string(),
        max_messages: 100,
        tools: vec![],
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
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_valid() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 100,
        tools: vec![],
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_serialization() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 200,
        tools: vec!["search".to_string()],
    };
    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("model_name: gpt-4"));
    let deserialized: Config = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(deserialized.model_name, "gpt-4");
}
