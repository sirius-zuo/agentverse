//! Integration tests for StrategyRouter.
//!
//! NOTE: These tests are partially disabled because ProviderWrapper::generate()
//! is a stub that always returns an error (will be replaced by ConnectionManager in Task 2).
//! Structural and non-routing tests remain active.

use agentverse_router::{StrategyName, StrategyRouter};

fn make_router(response: &str) -> StrategyRouter {
    let _ = response; // unused until ConnectionManager replaces the stub
    let model = agentverse::ProviderWrapper::openai("https://api.openai.com/v1", "gpt-4o", "test-key");
    StrategyRouter::new(
        model,
        vec![
            StrategyName::ReAct,
            StrategyName::PlanAndExecute,
            StrategyName::Hierarchical,
        ],
    )
}

/// Routing tests are disabled until ProviderWrapper::generate() is implemented
/// by ConnectionManager (Task 2). Until then, every call returns ApiError.
#[tokio::test]
async fn test_route_returns_error_until_connection_manager() {
    let router = make_router("react");
    let result = router.route("What is 2+2?").await;
    // ProviderWrapper::generate() is a stub — expect an error for now.
    assert!(result.is_err());
}

#[tokio::test]
async fn test_available_strategies() {
    let router = make_router("react");
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
