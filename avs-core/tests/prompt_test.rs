use agentverse::PromptRegistry;
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
