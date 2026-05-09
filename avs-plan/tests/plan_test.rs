//! Tests for the agentverse-plan crate.
//!
//! Tests the PlanStep, Plan, and strategy structures with mock models.

use agentverse::model::ToolDefinition;
use agentverse::{ModelError, ModelProvider, SyncTool};
use agentverse_plan::{Plan, PlanStep};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

// ─── Mock types ───────────────────────────────────────────────────────────────

/// A mock model that returns pre-configured responses.
struct MockModel {
    responses: Vec<String>,
    index: AtomicUsize,
}

#[async_trait::async_trait]
impl ModelProvider for MockModel {
    async fn generate(
        &self,
        _prompt: &str,
        _tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError> {
        let idx = self.index.fetch_add(1, Ordering::SeqCst) % self.responses.len();
        Ok(self.responses[idx].clone())
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

    // First call
    let resp1 = model.generate("prompt", None).await.unwrap();
    assert_eq!(resp1, "Hello");

    // Second call
    let resp2 = model.generate("prompt", None).await.unwrap();
    assert_eq!(resp2, "World");

    // Third call (cycles back)
    let resp3 = model.generate("prompt", None).await.unwrap();
    assert_eq!(resp3, "Hello");
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
