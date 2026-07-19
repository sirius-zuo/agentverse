//! Integration tests for StrategyRouter.

use agentverse::{AgentError, Config, LlmRunner, ModelError};
use agentverse_router::{StrategyName, StrategyRouter};
use httpmock::prelude::*;
use std::sync::Arc;

fn make_router() -> StrategyRouter {
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "gpt-4o".to_string(),
                "test-key".to_string(),
                Some("https://api.openai.com/v1".to_string()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    StrategyRouter::new(
        runner,
        vec![
            StrategyName::ReAct,
            StrategyName::PlanAndExecute,
            StrategyName::Hierarchical,
        ],
    )
}

/// Routing test: a real HTTP call would be needed for success.
/// Verify that the router wires up correctly and fails gracefully without a real server.
#[tokio::test]
async fn test_route_fails_without_server() {
    let router = make_router();
    let result = router.route("What is 2+2?").await;
    // No real server — expect an error (network or API error).
    assert!(result.is_err());
}

#[tokio::test]
async fn test_available_strategies() {
    let router = make_router();
    let strategies = router.available_strategies();
    assert_eq!(strategies.len(), 3);
    assert!(strategies.contains(&StrategyName::ReAct));
    assert!(strategies.contains(&StrategyName::PlanAndExecute));
    assert!(strategies.contains(&StrategyName::Hierarchical));
}

#[test]
fn test_strategy_display() {
    assert_eq!(StrategyName::ReAct.to_string(), "react");
    assert_eq!(StrategyName::PlanAndExecute.to_string(), "plan_and_execute");
    assert_eq!(StrategyName::Hierarchical.to_string(), "hierarchical");
}

#[tokio::test]
async fn strategy_router_rejects_model_selection_outside_allowlist() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "plan_and_execute"}}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
            }));
        })
        .await;
    let router = StrategyRouter::new(
        Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::openai(
                    "test",
                    "test-key",
                    Some(server.base_url()),
                ),
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        ),
        vec![StrategyName::ReAct],
    );

    let error = router.route("make a plan").await.unwrap_err();

    assert!(matches!(
        error,
        AgentError::Model(ModelError::InvalidResponse(ref message))
            if message.contains("not in the router's available strategies")
    ));
}
