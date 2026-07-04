//! Integration tests for StrategyRouter.

use agentverse::{Config, LlmRunner};
use agentverse_router::{StrategyName, StrategyRouter};
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
