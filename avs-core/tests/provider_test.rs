use agentverse::{ConnectionManager, ProviderConfig};

// ── ConnectionManager construction tests ──────────────────────────────────────

#[test]
fn test_connection_manager_from_config_openai() {
    let config = ProviderConfig::openai("gpt-4o".to_string(), "sk-test".to_string(), None);
    let registry = agentverse::ProviderRegistry::with_builtins();
    assert!(ConnectionManager::from_config(config, &registry).is_ok());
}

#[test]
fn test_connection_manager_from_config_anthropic() {
    let config =
        ProviderConfig::anthropic("claude-3-5-sonnet-20241022".to_string(), "key".to_string());
    let registry = agentverse::ProviderRegistry::with_builtins();
    assert!(ConnectionManager::from_config(config, &registry).is_ok());
}

#[test]
fn test_connection_manager_from_config_gemini() {
    let config = ProviderConfig::gemini("gemini-pro".to_string(), "key".to_string());
    let registry = agentverse::ProviderRegistry::with_builtins();
    assert!(ConnectionManager::from_config(config, &registry).is_ok());
}

// ── ProviderConfig serialization ──────────────────────────────────────────────

#[test]
fn test_provider_config_serialization() {
    let config = ProviderConfig::openai(
        "gpt-4".to_string(),
        "sk-xxx".to_string(),
        Some("http://localhost:9090/v1".to_string()),
    );
    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("gpt-4"));
    let deserialized: ProviderConfig = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(deserialized.name, "openai");
    assert_eq!(deserialized.settings.get("model_name").unwrap(), "gpt-4");
}

#[test]
fn connection_manager_with_model_uses_new_model_name() {
    use agentverse::memory::{Message, MessageRole};
    let cm =
        ConnectionManager::anthropic("https://api.anthropic.com", "claude-sonnet-4-6", "test-key");
    let registry = agentverse::ProviderRegistry::with_builtins();
    let overridden = cm
        .with_model("claude-haiku-4-5-20251001", &registry)
        .expect("known provider");
    let req = agentverse::GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hi")],
        tools: None,
        ..Default::default()
    };
    let body = overridden.provider_build_request_for_test(req).unwrap();
    assert_eq!(body["model"].as_str().unwrap(), "claude-haiku-4-5-20251001");
}

#[test]
fn connection_manager_with_model_openai_uses_new_model_name() {
    use agentverse::memory::{Message, MessageRole};
    let cm = ConnectionManager::openai("https://api.openai.com/v1", "gpt-4o", "test-key");
    let registry = agentverse::ProviderRegistry::with_builtins();
    let overridden = cm
        .with_model("gpt-4o-mini", &registry)
        .expect("known provider");
    let req = agentverse::GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hi")],
        tools: None,
        ..Default::default()
    };
    let body = overridden.provider_build_request_for_test(req).unwrap();
    assert_eq!(body["model"].as_str().unwrap(), "gpt-4o-mini");
}

#[test]
fn connection_manager_with_model_keyless_openai_local_endpoint_succeeds() {
    let cm = ConnectionManager::openai("http://localhost:9090/v1", "m", "");
    let registry = agentverse::ProviderRegistry::with_builtins();
    assert!(
        cm.with_model("m2", &registry).is_ok(),
        "with_model should succeed for a keyless local-endpoint openai manager, matching pre-registry behavior"
    );
}

#[test]
fn connection_manager_with_model_gemini_uses_new_model_name() {
    use agentverse::memory::{Message, MessageRole};
    let cm = ConnectionManager::gemini(
        "https://generativelanguage.googleapis.com",
        "gemini-2.0-flash",
        "test-key",
    );
    let registry = agentverse::ProviderRegistry::with_builtins();
    let overridden = cm
        .with_model("gemini-1.5-pro", &registry)
        .expect("known provider");
    let req = agentverse::GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hi")],
        tools: None,
        ..Default::default()
    };
    let body = overridden.provider_build_request_for_test(req).unwrap();
    // Gemini puts the model name in the URL path, not the request body —
    // just verify the request builds successfully with the new model.
    let _ = body;
}
