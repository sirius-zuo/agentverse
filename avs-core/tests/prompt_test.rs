use agentverse::{Example, PromptConfig, PromptRegistry};
use std::collections::HashMap;

#[test]
fn test_prompt_registry_has_default_react_template() {
    let registry = PromptRegistry::new();
    let mut ctx = HashMap::new();
    ctx.insert("tools".to_string(), serde_json::json!(""));
    ctx.insert("conversation".to_string(), serde_json::json!(""));
    let result = registry.render("react", ctx);
    assert!(result.is_ok());
    let rendered = result.unwrap();
    assert!(rendered.contains("ReAct pattern"));
    assert!(rendered.contains("Thought:"));
    assert!(rendered.contains("Action:"));
}

#[test]
fn test_prompt_registry_unknown_template() {
    let registry = PromptRegistry::new();
    let result = registry.render("nonexistent", HashMap::new());
    assert!(result.is_err());
}

#[test]
fn test_prompt_registry_add_custom_template() {
    let mut registry = PromptRegistry::new();
    registry.add_template("custom", "Hello {{ name }}!");
    let mut ctx = HashMap::new();
    ctx.insert("name".to_string(), serde_json::json!("World"));
    let result = registry.render("custom", ctx);
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Hello World"));
}

#[test]
fn test_prompt_registry_default() {
    let registry = PromptRegistry::default();
    let mut ctx = HashMap::new();
    ctx.insert("tools".to_string(), serde_json::json!(""));
    ctx.insert("conversation".to_string(), serde_json::json!(""));
    let result = registry.render("react", ctx);
    assert!(result.is_ok());
}

#[test]
fn test_prompt_registry_has_system_template() {
    let registry = PromptRegistry::new();
    let mut ctx = HashMap::new();
    ctx.insert("tools".to_string(), serde_json::json!(""));
    ctx.insert("conversation".to_string(), serde_json::json!(""));
    let result = registry.render("system", ctx);
    assert!(result.is_ok());
    let rendered = result.unwrap();
    assert!(rendered.contains("helpful AI assistant"));
}

#[test]
fn test_prompt_registry_has_all_strategy_templates() {
    let registry = PromptRegistry::new();
    let mut ctx = HashMap::new();
    ctx.insert("tools".to_string(), serde_json::json!(""));
    ctx.insert("conversation".to_string(), serde_json::json!(""));

    // All strategy templates should render
    assert!(registry.render("strategies.react", ctx.clone()).is_ok());
    assert!(registry
        .render("strategies.plan_and_execute", ctx.clone())
        .is_ok());
    assert!(registry
        .render("strategies.hierarchical.decompose", ctx)
        .is_ok());
}

// ─── from_config / load_from_directory tests ─────────────────────────────────

#[test]
fn test_from_config_nonexistent_dir() {
    let result = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some("/nonexistent/path/that/does/not/exist".to_string()),
        ..Default::default()
    });
    assert!(result.is_err());
    assert!(result.err().unwrap().to_string().contains("not found"));
}

#[test]
fn test_from_config_loads_j2_template() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("system.j2"), "Custom system: {{ tools }}").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    let mut ctx = HashMap::new();
    ctx.insert("tools".to_string(), serde_json::json!("calculator"));
    let rendered = registry.render("system", ctx).unwrap();
    assert!(rendered.contains("Custom system: calculator"));
}

#[test]
fn test_from_config_react_j2_sets_flag() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("react.j2"), "React preamble: {{ tools }}").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(registry.has_react_template());
}

#[test]
fn test_from_config_no_react_j2_flag_false() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("system.j2"), "System only").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(!registry.has_react_template());
}

#[test]
fn test_from_config_loads_toml_examples() {
    let dir = tempfile::tempdir().unwrap();
    let toml = "[[example]]\ninput = \"add 2+2\"\noutput = \"4\"\n";
    std::fs::write(dir.path().join("react_examples.toml"), toml).unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    let examples = registry.get_examples("react_examples").unwrap();
    assert_eq!(examples.len(), 1);
    assert_eq!(examples[0].input, "add 2+2");
    assert_eq!(examples[0].output, Some("4".to_string()));
}

#[test]
fn test_from_config_invalid_toml_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("react_examples.toml"),
        "not valid toml = [[[",
    )
    .unwrap();

    let result = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    });

    assert!(result.is_err());
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("Cannot parse examples"));
}

#[test]
fn test_from_config_plan_and_execute_key_mapping() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("plan_and_execute.j2"),
        "Plan prompt: {{ tools }}",
    )
    .unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    let mut ctx = HashMap::new();
    ctx.insert("tools".to_string(), serde_json::json!("calculator"));
    // Must be registered under the canonical key, not the file stem
    let rendered = registry.render("strategies.plan_and_execute", ctx).unwrap();
    assert!(rendered.contains("Plan prompt: calculator"));
}

#[test]
fn test_from_config_hierarchical_key_mapping() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("hierarchical.j2"),
        "Decompose: {{ conversation }}",
    )
    .unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    let mut ctx = HashMap::new();
    ctx.insert("conversation".to_string(), serde_json::json!("task"));
    let rendered = registry
        .render("strategies.hierarchical.decompose", ctx)
        .unwrap();
    assert!(rendered.contains("Decompose: task"));
}

#[test]
fn test_from_config_system_prompt_override() {
    let registry = PromptRegistry::from_config(&PromptConfig {
        system_prompt: Some("You are a pirate.".to_string()),
        ..Default::default()
    })
    .unwrap();

    let rendered = registry.render("system", HashMap::new()).unwrap();
    assert_eq!(rendered, "You are a pirate.");
}

#[test]
fn test_from_config_programmatic_templates() {
    let mut templates = HashMap::new();
    templates.insert("greeting".to_string(), "Hello, {{ name }}!".to_string());
    let registry = PromptRegistry::from_config(&PromptConfig {
        templates,
        ..Default::default()
    })
    .unwrap();

    let mut ctx = HashMap::new();
    ctx.insert("name".to_string(), serde_json::json!("World"));
    let rendered = registry.render("greeting", ctx).unwrap();
    assert_eq!(rendered, "Hello, World!");
}

#[test]
fn test_from_config_programmatic_examples() {
    let mut examples = HashMap::new();
    examples.insert(
        "my_examples".to_string(),
        vec![Example {
            input: "q".to_string(),
            output: Some("a".to_string()),
            strategy: None,
        }],
    );
    let registry = PromptRegistry::from_config(&PromptConfig {
        examples,
        ..Default::default()
    })
    .unwrap();

    let ex = registry.get_examples("my_examples").unwrap();
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0].input, "q");
    assert_eq!(ex[0].output, Some("a".to_string()));
}
