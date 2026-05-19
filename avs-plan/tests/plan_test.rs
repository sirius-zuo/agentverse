//! Tests for the agentverse-plan crate.
//!
//! Tests the PlanStep, Plan, and strategy structures with mock models.

use agentverse::memory::{Message, MessageRole};
use agentverse::{
    GenerateRequest, GenerateResponse, ModelError, ModelProvider, PromptRegistry, SyncTool,
    UsageStats,
};
use agentverse_memory::SimpleMemory;
use agentverse_plan::{HierarchicalStrategy, Plan, PlanStep, PlanStrategy};
use agentverse_tools::{SyncToolAdapter, ToolRegistry};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

// ─── Mock types ───────────────────────────────────────────────────────────────

/// A mock model that returns pre-configured responses.
struct MockModel {
    responses: Vec<String>,
    index: AtomicUsize,
}

#[async_trait::async_trait]
impl ModelProvider for MockModel {
    fn name(&self) -> &str {
        "mock-model"
    }

    async fn generate(&self, _request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        let idx = self.index.fetch_add(1, Ordering::SeqCst) % self.responses.len();
        Ok(GenerateResponse {
            content: self.responses[idx].clone(),
            usage: UsageStats::default(),
        })
    }
}

/// A mock sync tool.
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

impl SyncTool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }

    fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, agentverse::ToolError> {
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

    let result = tool.execute(json!({"key": "value"})).unwrap();
    assert!(result.is_object());
}

#[tokio::test]
async fn test_mock_model_responses() {
    let model = MockModel {
        responses: vec![
            "Hello".to_string(),
            "World".to_string(),
            "Hello".to_string(), // Cycles
        ],
        index: AtomicUsize::new(0),
    };

    let req = || GenerateRequest {
        system: None,
        messages: vec![Message {
            role: MessageRole::User,
            content: "prompt".to_string(),
        }],
        tools: None,
    };

    // First call
    let resp1 = model.generate(req()).await.unwrap();
    assert_eq!(resp1.content, "Hello");

    // Second call
    let resp2 = model.generate(req()).await.unwrap();
    assert_eq!(resp2.content, "World");

    // Third call (cycles back)
    let resp3 = model.generate(req()).await.unwrap();
    assert_eq!(resp3.content, "Hello");
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

    // Verify dependency chain
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

// ─── Planner function tests ───────────────────────────────────────────────────

fn simple_plan_json() -> String {
    r#"{"description":"Test plan","steps":[{"id":1,"description":"Reason about it","depends_on":[]}]}"#
        .to_string()
}

fn tool_plan_json() -> String {
    r#"{"description":"Tool plan","steps":[{"id":1,"description":"Run tool","tool":"echo","depends_on":[]}]}"#
        .to_string()
}

#[tokio::test]
async fn test_generate_plan_success() {
    let model = MockModel {
        responses: vec![simple_plan_json()],
        index: AtomicUsize::new(0),
    };
    let registry = PromptRegistry::default();
    let plan = agentverse_plan::planner::generate_plan(&model, &registry, "do something", &[], "")
        .await
        .unwrap();
    assert_eq!(plan.description, "Test plan");
    assert_eq!(plan.steps.len(), 1);
}

#[tokio::test]
async fn test_generate_plan_invalid_json() {
    let model = MockModel {
        responses: vec!["not valid json at all".to_string()],
        index: AtomicUsize::new(0),
    };
    let registry = PromptRegistry::default();
    let err = agentverse_plan::planner::generate_plan(&model, &registry, "do something", &[], "")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Failed to parse plan JSON"));
}

#[tokio::test]
async fn test_generate_plan_with_tools() {
    let model = MockModel {
        responses: vec![tool_plan_json()],
        index: AtomicUsize::new(0),
    };
    let registry = PromptRegistry::default();
    let plan = agentverse_plan::planner::generate_plan(
        &model,
        &registry,
        "do something",
        &["echo".to_string()],
        "User: do something",
    )
    .await
    .unwrap();
    assert_eq!(plan.steps[0].tool, Some("echo".to_string()));
}

#[tokio::test]
async fn test_decompose_request_success() {
    let model = MockModel {
        responses: vec![r#"["Sub-goal 1","Sub-goal 2"]"#.to_string()],
        index: AtomicUsize::new(0),
    };
    let registry = PromptRegistry::default();
    let sub_goals = agentverse_plan::planner::decompose_request(&model, &registry, "big request")
        .await
        .unwrap();
    assert_eq!(sub_goals, vec!["Sub-goal 1", "Sub-goal 2"]);
}

#[tokio::test]
async fn test_decompose_request_invalid_json() {
    let model = MockModel {
        responses: vec!["not json".to_string()],
        index: AtomicUsize::new(0),
    };
    let registry = PromptRegistry::default();
    let err = agentverse_plan::planner::decompose_request(&model, &registry, "big request")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Failed to parse decomposition"));
}

// ─── PlanStrategy::run() tests ───────────────────────────────────────────────

#[tokio::test]
async fn test_plan_strategy_reasoning_steps() {
    let model = MockModel {
        responses: vec![simple_plan_json(), "Final answer.".to_string()],
        index: AtomicUsize::new(0),
    };
    let mut strategy = PlanStrategy::new(
        Arc::new(model),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(20))),
        10,
    );
    let result = strategy.run("do something".to_string()).await.unwrap();
    assert_eq!(result, "Final answer.");
}

#[tokio::test]
async fn test_plan_strategy_tool_steps() {
    let model = MockModel {
        responses: vec![tool_plan_json(), "Final answer.".to_string()],
        index: AtomicUsize::new(0),
    };
    let tool = MockTool::new("echo", "Echo tool");
    let mut registry = ToolRegistry::new();
    registry.register(SyncToolAdapter(tool));
    let mut strategy = PlanStrategy::new(
        Arc::new(model),
        Arc::new(PromptRegistry::default()),
        registry,
        Arc::new(Mutex::new(SimpleMemory::new(20))),
        10,
    );
    let result = strategy.run("use echo".to_string()).await.unwrap();
    assert_eq!(result, "Final answer.");
}

#[tokio::test]
async fn test_plan_strategy_tool_not_found_graceful() {
    // Plan references "missing_tool" but no tool is registered — should not panic
    let plan_with_unknown = r#"{"description":"Bad tool","steps":[{"id":1,"description":"Use missing","tool":"missing_tool","depends_on":[]}]}"#.to_string();
    let model = MockModel {
        responses: vec![plan_with_unknown, "Graceful answer.".to_string()],
        index: AtomicUsize::new(0),
    };
    let mut strategy = PlanStrategy::new(
        Arc::new(model),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(20))),
        10,
    );
    let result = strategy.run("test".to_string()).await.unwrap();
    assert_eq!(result, "Graceful answer.");
}

#[tokio::test]
async fn test_plan_strategy_max_iterations_skips_steps() {
    // max_iterations=0 means every step (id>0) is skipped
    let plan = r#"{"description":"Big plan","steps":[{"id":1,"description":"Step 1","depends_on":[]},{"id":2,"description":"Step 2","depends_on":[]}]}"#.to_string();
    let model = MockModel {
        responses: vec![plan, "Short answer.".to_string()],
        index: AtomicUsize::new(0),
    };
    let mut strategy = PlanStrategy::new(
        Arc::new(model),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(20))),
        0,
    );
    let result = strategy.run("test".to_string()).await.unwrap();
    assert_eq!(result, "Short answer.");
}

// ─── HierarchicalStrategy::run() tests ───────────────────────────────────────

#[tokio::test]
async fn test_hierarchical_strategy_run() {
    let model = MockModel {
        responses: vec![
            r#"["Sub-goal 1"]"#.to_string(),    // decompose
            simple_plan_json(),                 // plan for sub-goal 1
            "Hierarchical answer.".to_string(), // synthesis
        ],
        index: AtomicUsize::new(0),
    };
    let mut strategy = HierarchicalStrategy::new(
        Arc::new(model),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(30))),
        10,
        5,
    );
    let result = strategy.run("complex task".to_string()).await.unwrap();
    assert_eq!(result, "Hierarchical answer.");
}

#[tokio::test]
async fn test_hierarchical_strategy_max_depth_limits_subgoals() {
    // decompose returns 2 sub-goals but max_decompose_depth=1 means only 1 is processed
    let model = MockModel {
        responses: vec![
            r#"["Sub-goal 1","Sub-goal 2"]"#.to_string(), // decompose
            simple_plan_json(),                           // plan for sub-goal 1 only
            "Depth-limited answer.".to_string(),          // synthesis
        ],
        index: AtomicUsize::new(0),
    };
    let mut strategy = HierarchicalStrategy::new(
        Arc::new(model),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(30))),
        10,
        1, // max_decompose_depth = 1
    );
    let result = strategy.run("complex task".to_string()).await.unwrap();
    assert_eq!(result, "Depth-limited answer.");
}

#[tokio::test]
async fn test_hierarchical_strategy_zero_subgoals() {
    // decompose returns empty list — goes straight to synthesis
    let model = MockModel {
        responses: vec![
            r#"[]"#.to_string(),            // decompose: no sub-goals
            "Empty synthesis.".to_string(), // synthesis
        ],
        index: AtomicUsize::new(0),
    };
    let mut strategy = HierarchicalStrategy::new(
        Arc::new(model),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        Arc::new(Mutex::new(SimpleMemory::new(30))),
        10,
        5,
    );
    let result = strategy.run("simple task".to_string()).await.unwrap();
    assert_eq!(result, "Empty synthesis.");
}

#[tokio::test]
async fn test_plan_strategy_accepts_tool_registry() {
    use agentverse_tools::{Calculator, SyncToolAdapter, ToolRegistry};
    // Just verify construction compiles and registry is accessible
    let mut tools = ToolRegistry::new();
    tools.register(SyncToolAdapter(Calculator));
    assert!(tools.has_tool("calculator"));
}
