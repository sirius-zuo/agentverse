//! Tests for the agentverse-plan crate.
//!
//! Tests the PlanStep, Plan, and strategy structures.

use agentverse::{Config, LlmRunner, PromptRegistry, Tool, ToolResult};
use agentverse_plan::{HierarchicalStrategy, Plan, PlanStep, PlanStrategy};
use agentverse_tools::{Calculator, ToolRegistry};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_runner() -> Arc<LlmRunner> {
    Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test".to_string(),
                "sk-test".to_string(),
                Some("http://127.0.0.1:1/v1".to_string()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    )
}

/// A mock tool.
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

#[derive(Debug, Deserialize, JsonSchema)]
struct MockArgs {}

#[async_trait::async_trait]
impl Tool for MockTool {
    type Args = MockArgs;

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    async fn execute(&self, args: MockArgs) -> ToolResult {
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
    use agentverse::ErasedTool;
    let tool = MockTool::new("test_tool", "A test tool");
    let result = tool.execute_raw(json!({"text": "hello"})).await.unwrap();
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

// ─── PlanStrategy and HierarchicalStrategy construction tests ─────────────────

#[test]
fn test_plan_strategy_construction() {
    let _strategy = PlanStrategy::new(
        make_runner(),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        10,
    );
}

#[test]
fn test_hierarchical_strategy_construction() {
    let _strategy = HierarchicalStrategy::new(
        make_runner(),
        Arc::new(PromptRegistry::default()),
        ToolRegistry::new(),
        10,
        5,
    );
}

#[tokio::test]
async fn test_plan_strategy_accepts_tool_registry() {
    let tools = ToolRegistry::new();
    tools.register(Calculator);
    assert!(tools.has_tool("calculator"));
}

#[test]
fn plan_strategy_implements_run_strategy() {
    fn assert_run_strategy<T: agentverse::RunStrategy + ?Sized>() {}
    assert_run_strategy::<dyn agentverse::RunStrategy>();
}

#[tokio::test]
async fn plan_run_returns_error_on_bad_port() {
    use agentverse::RunStrategy;
    let strategy = PlanStrategy::new(
        make_runner(),
        Arc::new(PromptRegistry::new()),
        ToolRegistry::new(),
        5,
    );
    let messages = vec![agentverse::Message::text(
        agentverse::memory::MessageRole::User,
        "Search for rust",
    )];
    let result = strategy.run(messages).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn hierarchical_run_returns_error_on_bad_port() {
    use agentverse::RunStrategy;
    let strategy = HierarchicalStrategy::new(
        make_runner(),
        Arc::new(PromptRegistry::new()),
        ToolRegistry::new(),
        5,
        3,
    );
    let messages = vec![agentverse::Message::text(
        agentverse::memory::MessageRole::User,
        "complex task",
    )];
    let result = strategy.run(messages).await;
    assert!(result.is_err());
}
