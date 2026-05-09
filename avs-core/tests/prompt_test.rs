use agentverse::PromptRegistry;
use std::collections::HashMap;

#[test]
fn test_prompt_registry_has_default_react_template() {
    let registry = PromptRegistry::new();
    let mut ctx = HashMap::new();
    ctx.insert("name".to_string(), "AgentVerse".to_string());
    let result = registry.render("react", ctx);
    assert!(result.is_ok());
    let rendered = result.unwrap();
    assert!(rendered.contains("helpful assistant"));
    assert!(rendered.contains("Think step by step"));
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
    ctx.insert("name".to_string(), "World".to_string());
    let result = registry.render("custom", ctx);
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Hello World"));
}

#[test]
fn test_prompt_registry_default() {
    let registry = PromptRegistry::default();
    let result = registry.render("react", HashMap::new());
    assert!(result.is_ok());
}
