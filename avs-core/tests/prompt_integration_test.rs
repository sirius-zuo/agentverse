use agentverse::{Example, PromptConfig, PromptRegistry};
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_prompt_rendering_with_examples() {
    let mut registry = PromptRegistry::new();
    registry.add_examples(
        "test_examples".to_string(),
        vec![Example {
            input: "What is 2+2?".to_string(),
            output: Some("Answer: 4".to_string()),
            strategy: None,
        }],
    );

    let mut context = HashMap::new();
    context.insert(
        "examples".to_string(),
        json!(registry.get_examples("test_examples")),
    );
    context.insert("tools".to_string(), json!(""));
    context.insert("conversation".to_string(), json!(""));

    let result = registry.render("strategies.react", context).unwrap();
    assert!(result.contains("Answer: 4"));
    assert!(result.contains("reasoning step by step"));
}

#[test]
fn test_prompt_rendering_without_examples() {
    let registry = PromptRegistry::new();
    let mut context = HashMap::new();
    context.insert("examples".to_string(), json!(None::<Vec<Example>>));
    context.insert("tools".to_string(), json!(""));
    context.insert("conversation".to_string(), json!(""));

    let result = registry.render("strategies.react", context).unwrap();
    assert!(result.contains("reasoning step by step"));
    // Should not contain "Examples:" since examples is empty
    assert!(!result.contains("Examples:"));
}

#[test]
fn test_router_prompt_rendering() {
    let mut registry = PromptRegistry::new();
    registry.add_examples(
        "router_examples".to_string(),
        vec![Example {
            input: "What time is it?".to_string(),
            output: None,
            strategy: Some("react".to_string()),
        }],
    );

    let mut context = HashMap::new();
    context.insert(
        "examples".to_string(),
        json!(registry.get_examples("router_examples")),
    );
    context.insert("conversation".to_string(), json!(""));
    context.insert("tools".to_string(), json!(""));

    let result = registry.render("router", context).unwrap();
    assert!(result.contains("Choose the best orchestration strategy"));
    assert!(result.contains("react"));
}

#[test]
fn test_plan_prompt_rendering() {
    let mut registry = PromptRegistry::new();
    registry.add_examples(
        "plan_examples".to_string(),
        vec![Example {
            input: "Search for weather".to_string(),
            output: Some("Plan: search_weather".to_string()),
            strategy: None,
        }],
    );

    let mut context = HashMap::new();
    context.insert(
        "examples".to_string(),
        json!(registry.get_examples("plan_examples")),
    );
    context.insert("tools".to_string(), json!("weather, search"));
    context.insert("conversation".to_string(), json!(""));

    let result = registry
        .render("strategies.plan_and_execute", context)
        .unwrap();
    assert!(result.contains("planning assistant"));
    assert!(result.contains("search_weather"));
}

#[test]
fn test_system_prompt_override() {
    let config = PromptConfig {
        system_prompt: Some("Custom system prompt".to_string()),
        prompts_dir: None,
        templates: std::collections::HashMap::new(),
        examples: std::collections::HashMap::new(),
    };

    let registry = PromptRegistry::from_config(&config).unwrap();
    let mut context = HashMap::new();
    context.insert("conversation".to_string(), json!(""));
    context.insert("tools".to_string(), json!(""));

    let result = registry.render("system", context).unwrap();
    assert_eq!(result, "Custom system prompt");
}

#[test]
fn test_default_system_prompt() {
    let registry = PromptRegistry::new();
    let mut context = HashMap::new();
    context.insert("conversation".to_string(), json!(""));
    context.insert("tools".to_string(), json!(""));

    let result = registry.render("system", context).unwrap();
    assert!(result.contains("helpful AI assistant"));
}
