use agentverse_subagent::*;
use std::time::Duration;

#[test]
fn budget_round_trips_through_json() {
    let b = Budget {
        max_steps: 10,
        max_tokens: 5000,
        timeout: Duration::from_secs(60),
    };
    let json = serde_json::to_string(&b).unwrap();
    let b2: Budget = serde_json::from_str(&json).unwrap();
    assert_eq!(b2.max_steps, 10);
    assert_eq!(b2.max_tokens, 5000);
    assert_eq!(b2.timeout, Duration::from_secs(60));
}

#[test]
fn subagent_spec_defaults_model_to_none() {
    let spec = SubAgentSpec {
        name: "test".into(),
        objective: "do thing".into(),
        system_prompt: None,
        model: None,
        allowed_tools: vec![],
        budget: Budget { max_steps: 5, max_tokens: 1000, timeout: Duration::from_secs(30) },
    };
    assert!(spec.model.is_none());
    assert!(spec.allowed_tools.is_empty());
}

#[test]
fn model_override_alias_and_id_round_trip() {
    let alias = ModelOverride::Alias("haiku".into());
    let id    = ModelOverride::Id("claude-haiku-4-5-20251001".into());
    let j1 = serde_json::to_string(&alias).unwrap();
    let j2 = serde_json::to_string(&id).unwrap();
    let alias2: ModelOverride = serde_json::from_str(&j1).unwrap();
    let id2: ModelOverride    = serde_json::from_str(&j2).unwrap();
    assert!(matches!(alias2, ModelOverride::Alias(_)));
    assert!(matches!(id2,    ModelOverride::Id(_)));
}

#[test]
fn subagent_context_resources_accessible() {
    let ctx = SubAgentContext {
        resources: vec![
            ResourceContent { label: "main.rs".into(), content: "fn main() {}".into() },
        ],
        depth: 0,
    };
    assert_eq!(ctx.resources[0].label, "main.rs");
    assert_eq!(ctx.depth, 0);
}

#[test]
fn subagent_result_fields() {
    let r = SubAgentResult {
        answer: "done".into(),
        usage: agentverse::UsageStats { input_tokens: 10, output_tokens: 5, ..Default::default() },
        steps: 3,
    };
    assert_eq!(r.answer, "done");
    assert_eq!(r.steps, 3);
    assert_eq!(r.usage.input_tokens, 10);
}

#[test]
fn subagent_error_display_depth_exceeded() {
    let e = SubAgentError::DepthExceeded;
    assert!(e.to_string().contains("depth"));
}

#[test]
fn subagent_error_display_step_budget() {
    let e = SubAgentError::StepBudgetExceeded { steps: 7 };
    assert!(e.to_string().contains("7"));
}

#[test]
fn subagent_error_display_token_budget() {
    let e = SubAgentError::TokenBudgetExceeded { used: 5001, limit: 5000 };
    assert!(e.to_string().contains("5001"));
}
