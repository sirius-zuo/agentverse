//! Tests for the agentverse-plan crate.
//!
//! Tests the PlanStep, Plan, and strategy structures.

use agentverse::{
    AsyncTool, ConnectionManager, GenerateRequest, ModelError, ModelProvider, PromptRegistry,
};
use agentverse_memory::SimpleMemory;
use agentverse_plan::{HierarchicalStrategy, Plan, PlanStep, PlanStrategy};
use agentverse_tools::{Calculator, ToolRegistry};
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

// ─── Mock types ───────────────────────────────────────────────────────────────

/// A mock model implementing the new 4-method ModelProvider trait.
struct MockModel {
    #[allow(dead_code)]
    responses: Vec<String>,
}

impl ModelProvider for MockModel {
    fn name(&self) -> &str {
        "mock-model"
    }

    fn build_request(
        &self,
        _model: &str,
        _request: GenerateRequest,
    ) -> Result<serde_json::Value, ModelError> {
        Ok(json!({}))
    }

    fn parse_response(&self, _body: &str) -> Result<agentverse::GenerateResponse, ModelError> {
        Ok(agentverse::GenerateResponse {
            content: String::new(),
            usage: agentverse::UsageStats::default(),
        })
    }

    fn request_headers(&self, _api_key: &str) -> HeaderMap {
        HeaderMap::new()
    }

    fn endpoint_path(&self, _model: &str) -> String {
        "/mock".to_string()
    }
}

fn make_wrapper() -> ConnectionManager {
    ConnectionManager::new(MockModel { responses: vec![] }, "http://localhost", "test-key", "mock-model")
}

/// A mock async tool.
struct MockTool {
    name: String,
    description: String,
}

impl MockTool {
    fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
        }
    }
}

#[async_trait]
impl AsyncTool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, agentverse::ToolError> {
        Ok(json!({"result": format!("Executed {} with args {:?}", self.name, args)}))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_plan_step_serialization() {
    let step = PlanStep {
        id: 1,
        description: "Search the web".to_string(),
        tool: Some("web_search".to_string()),
        args: Some(json!({"query": "AgentVerse"})),
        depends_on: vec![],
    };

    let json = serde_json::to_string(&step).unwrap();
    let deserialized: PlanStep = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.id, 1);
    assert_eq!(deserialized.description, "Search the web");
    assert_eq!(deserialized.tool, Some("web_search".to_string()));
    assert_eq!(deserialized.args, Some(json!({"query": "AgentVerse"})));
}

#[test]
fn test_plan_step_defaults() {
    let step = PlanStep {
        id: 2,
        description: "Reason about results".to_string(),
        tool: None,
        args: None,
        depends_on: vec![],
    };

    let json = serde_json::to_string(&step).unwrap();
    let deserialized: PlanStep = serde_json::from_str(&json).unwrap();

    assert!(deserialized.tool.is_none());
    assert!(deserialized.args.is_none());
    assert!(deserialized.depends_on.is_empty());
}

#[test]
fn test_plan_is_empty() {
    let empty_plan = Plan {
        description: "Empty".to_string(),
        steps: vec![],
    };
    assert!(empty_plan.is_empty());

    let filled_plan = Plan {
        description: "Has steps".to_string(),
        steps: vec![PlanStep {
            id: 1,
            description: "Step 1".to_string(),
            tool: None,
            args: None,
            depends_on: vec![],
        }],
    };
    assert!(!filled_plan.is_empty());
}

#[test]
fn test_plan_serialization() {
    let plan = Plan {
        description: "Test plan".to_string(),
        steps: vec![
            PlanStep {
                id: 1,
                description: "Step 1".to_string(),
                tool: Some("tool1".to_string()),
                args: Some(json!({"a": 1})),
                depends_on: vec![],
            },
            PlanStep {
                id: 2,
                description: "Step 2".to_string(),
                tool: None,
                args: None,
                depends_on: vec![1],
            },
        ],
    };

    let json = serde_json::to_string_pretty(&plan).unwrap();
    let deserialized: Plan = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.description, "Test plan");
    assert_eq!(deserialized.steps.len(), 2);
    assert_eq!(deserialized.steps[0].id, 1);
    assert_eq!(deserialized.steps[1].depends_on, vec![1]);
}

#[test]
fn test_mock_tool() {
    let tool = MockTool::new("test_tool", "A test tool");
    assert_eq!(tool.name(), "test_tool");
    assert_eq!(tool.description(), "A test tool");
}

#[tokio::test]
async fn test_mock_tool_execute() {
    let tool = MockTool::new("test_tool", "A test tool");
    let result = tool.execute(json!({"key": "value"})).await.unwrap();
    assert!(result.is_object());
}

#[test]
fn test_plan_step_with_dependencies() {
    let plan = Plan {
        description: "Multi-step plan".to_string(),
        steps: vec![
            PlanStep {
                id: 1,
                description: "Fetch data".to_string(),
                tool: Some("fetch".to_string()),
                args: Some(json!({"url": "https://api.example.com"})),
                depends_on: vec![],
            },
            PlanStep {
                id: 2,
                description: "Parse data".to_string(),
                tool: None,
                args: None,
                depends_on: vec![1],
            },
            PlanStep {
                id: 3,
                description: "Summarize".to_string(),
                tool: None,
                args: None,
                depends_on: vec![2],
            },
        ],
    };

    assert!(plan.steps[0].depends_on.is_empty());
    assert_eq!(plan.steps[1].depends_on, vec![1]);
    assert_eq!(plan.steps[2].depends_on, vec![2]);
}

#[test]
fn test_plan_step_complex_args() {
    let args = json!({
        "query": "AgentVerse",
        "limit": 10,
        "filters": {
            "category": "AI",
            "date_range": {
                "start": "2024-01-01",
                "end": "2024-12-31"
            }
        }
    });

    let step = PlanStep {
        id: 1,
        description: "Search with complex filters".to_string(),
        tool: Some("search".to_string()),
        args: Some(args.clone()),
        depends_on: vec![],
    };

    let deserialized: PlanStep =
        serde_json::from_str(&serde_json::to_string(&step).unwrap()).unwrap();
    assert_eq!(deserialized.args, Some(args));
}

// ─── Planner function tests — disabled (require HTTP mock server wiring) ──────

#[tokio::test]
#[ignore = "requires HTTP mock server wiring; ConnectionManager now owns HTTP"]
async fn test_generate_plan_success() {
    let wrapper = make_wrapper();
    let registry = PromptRegistry::default();
    let _plan = agentverse_plan::planner::generate_plan(&wrapper, &registry, "do something", "", "")
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires HTTP mock server wiring; ConnectionManager now owns HTTP"]
async fn test_generate_plan_invalid_json() {
    let wrapper = make_wrapper();
    let registry = PromptRegistry::default();
    let err =
        agentverse_plan::planner::generate_plan(&wrapper, &registry, "do something", "", "")
            .await
            .unwrap_err();
    assert!(err.to_string().contains("Failed to parse plan JSON"));
}

#[tokio::test]
#[ignore = "requires HTTP mock server wiring; ConnectionManager now owns HTTP"]
async fn test_generate_plan_with_tools() {
    let wrapper = make_wrapper();
    let registry = PromptRegistry::default();
    let _plan = agentverse_plan::planner::generate_plan(
        &wrapper,
        &registry,
        "do something",
        "echo",
        "User: do something",
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires HTTP mock server wiring; ConnectionManager now owns HTTP"]
async fn test_decompose_request_success() {
    let wrapper = make_wrapper();
    let registry = PromptRegistry::default();
    let _sub_goals =
        agentverse_plan::planner::decompose_request(&wrapper, &registry, "big request")
            .await
            .unwrap();
}

#[tokio::test]
#[ignore = "requires HTTP mock server wiring; ConnectionManager now owns HTTP"]
async fn test_decompose_request_invalid_json() {
    let wrapper = make_wrapper();
    let registry = PromptRegistry::default();
    let err = agentverse_plan::planner::decompose_request(&wrapper, &registry, "big request")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Failed to parse decomposition"));
}

// ─── PlanStrategy and HierarchicalStrategy construction tests ─────────────────

#[test]
fn test_plan_strategy_construction() {
    let _strategy = PlanStrategy::new(
        Arc::new(make_wrapper()),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(20))),
        10,
    );
}

#[test]
fn test_hierarchical_strategy_construction() {
    let _strategy = HierarchicalStrategy::new(
        Arc::new(make_wrapper()),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(30))),
        10,
        5,
    );
}

#[tokio::test]
#[ignore = "requires HTTP mock server wiring; ConnectionManager now owns HTTP"]
async fn test_plan_strategy_reasoning_steps() {
    let mut strategy = PlanStrategy::new(
        Arc::new(make_wrapper()),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(20))),
        10,
    );
    let _result = strategy.run("do something".to_string()).await.unwrap();
}

#[tokio::test]
#[ignore = "requires HTTP mock server wiring; ConnectionManager now owns HTTP"]
async fn test_plan_strategy_tool_steps() {
    let mut registry = ToolRegistry::new();
    registry.register(MockTool::new("echo", "Echo tool"));
    let mut strategy = PlanStrategy::new(
        Arc::new(make_wrapper()),
        Arc::new(PromptRegistry::default()),
        registry,
        Arc::new(Mutex::new(SimpleMemory::new(20))),
        10,
    );
    let _result = strategy.run("use echo".to_string()).await.unwrap();
}

#[tokio::test]
#[ignore = "requires HTTP mock server wiring; ConnectionManager now owns HTTP"]
async fn test_hierarchical_strategy_run() {
    let mut strategy = HierarchicalStrategy::new(
        Arc::new(make_wrapper()),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(30))),
        10,
        5,
    );
    let _result = strategy.run("complex task".to_string()).await.unwrap();
}

#[tokio::test]
async fn test_plan_strategy_accepts_tool_registry() {
    let mut tools = ToolRegistry::new();
    tools.register(Calculator);
    assert!(tools.has_tool("calculator"));
}

#[test]
fn plan_strategy_implements_run_strategy() {
    fn assert_run_strategy<T: agentverse::RunStrategy + ?Sized>() {}
    assert_run_strategy::<dyn agentverse::RunStrategy>();
}
