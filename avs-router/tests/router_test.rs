//! Integration tests for StrategyRouter.

use agentverse::{GenerateRequest, GenerateResponse, ModelError, UsageStats};
use agentverse_router::{StrategyName, StrategyRouter};

/// Mock model provider for testing.
struct MockModel {
    response: String,
}

#[async_trait::async_trait]
impl agentverse::ModelProvider for MockModel {
    fn name(&self) -> &str {
        "mock-model"
    }

    async fn generate(
        &self,
        _request: GenerateRequest,
    ) -> Result<GenerateResponse, ModelError> {
        Ok(GenerateResponse {
            content: self.response.clone(),
            usage: UsageStats::default(),
        })
    }
}

fn make_router(response: &str) -> StrategyRouter<MockModel> {
    let model = MockModel {
        response: response.to_string(),
    };
    StrategyRouter::new(
        model,
        vec![
            StrategyName::ReAct,
            StrategyName::PlanAndExecute,
            StrategyName::Hierarchical,
        ],
    )
}

#[tokio::test]
async fn test_route_react() {
    let router = make_router("react");
    let result = router.route("What is 2+2?").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StrategyName::ReAct);
}

#[tokio::test]
async fn test_route_plan_and_execute() {
    let router = make_router("plan_and_execute");
    let result = router.route("Plan a trip to Paris").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StrategyName::PlanAndExecute);
}

#[tokio::test]
async fn test_route_plan_and_execute_hyphen() {
    let router = make_router("plan-and-execute");
    let result = router.route("Plan a trip to Paris").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StrategyName::PlanAndExecute);
}

#[tokio::test]
async fn test_route_hierarchical() {
    let router = make_router("hierarchical");
    let result = router.route("Analyze this complex dataset").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StrategyName::Hierarchical);
}

#[tokio::test]
async fn test_route_invalid_response() {
    let router = make_router("banana");
    let result = router.route("What is 2+2?").await;
    assert!(result.is_err());
    match result.unwrap_err() {
        agentverse::AgentError::Model(agentverse::ModelError::InvalidResponse(msg)) => {
            assert!(msg.contains("Unknown strategy"));
        }
        other => panic!("Expected InvalidResponse, got {:?}", other),
    }
}

#[tokio::test]
async fn test_route_case_insensitive() {
    let router = make_router("ReAct");
    let result = router.route("What is 2+2?").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StrategyName::ReAct);
}

#[tokio::test]
async fn test_route_with_whitespace() {
    let router = make_router("  hierarchical  ");
    let result = router.route("Analyze this complex dataset").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StrategyName::Hierarchical);
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

#[tokio::test]
async fn test_route_empty_response() {
    let router = make_router("");
    let result = router.route("What is 2+2?").await;
    assert!(result.is_err());
}
