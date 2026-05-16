//! Tests for the agentverse-react crate.

use agentverse::SyncTool;
use agentverse::{
    GenerateRequest, GenerateResponse, Memory, Message, ModelProvider, PromptRegistry,
    ShortTermMemory, UsageStats,
};
use agentverse_react::{parse::parse_response, CycleAction, ReActStrategy};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

    async fn generate(
        &self,
        _request: GenerateRequest,
    ) -> Result<GenerateResponse, agentverse::ModelError> {
        let idx = self.index.fetch_add(1, Ordering::SeqCst) % self.responses.len();
        Ok(GenerateResponse {
            content: self.responses[idx].clone(),
            usage: UsageStats::default(),
        })
    }
}

/// A mock sync tool for testing.
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

// ─── Parse tests ──────────────────────────────────────────────────────────────

#[test]
fn test_parse_response_answer() {
    let result = parse_response("Thought: done.\nAnswer: Hello world");
    match result {
        CycleAction::Done { answer } => assert_eq!(answer, "Hello world"),
        other => panic!("Expected Done, got {:?}", other),
    }
}

#[test]
fn test_parse_response_tool_call() {
    let result =
        parse_response("Thought: searching.\nAction: search\nAction Input: {\"q\": \"test\"}");
    match result {
        CycleAction::ToolCall { tool_name, args } => {
            assert_eq!(tool_name, "search");
            assert_eq!(args["q"], "test");
        }
        other => panic!("Expected ToolCall, got {:?}", other),
    }
}

#[test]
fn test_parse_response_thought_only() {
    let result = parse_response("I need to think about this first");
    match result {
        CycleAction::Continue { thought } => {
            assert_eq!(thought, "I need to think about this first");
        }
        other => panic!("Expected Continue, got {:?}", other),
    }
}

// ─── Cycle skeleton tests ─────────────────────────────────────────────────────

#[test]
fn test_cycle_skeleton_tool_count() {
    let tool = MockTool::new("test_tool", "A test tool");
    let skeleton = agentverse_react::CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![Box::new(tool)],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        10,
    );
    assert_eq!(skeleton.tool_count(), 1);
    assert_eq!(skeleton.max_iterations(), 10);
    assert_eq!(skeleton.current_iteration(), 0);
}

#[test]
fn test_cycle_skeleton_execute_tool() {
    let tool = MockTool::new("echo", "Echo back input");
    let skeleton = agentverse_react::CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![Box::new(tool)],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        10,
    );

    let result = skeleton
        .execute_tool("echo", json!({"text": "hello"}))
        .unwrap();
    assert!(result.contains("Executed echo"));
}

#[test]
fn test_cycle_skeleton_execute_tool_not_found() {
    let tool = MockTool::new("echo", "Echo back input");
    let skeleton = agentverse_react::CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![Box::new(tool)],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        10,
    );

    let result = skeleton.execute_tool("missing_tool", json!({}));
    assert!(result.is_err());
}

#[test]
fn test_cycle_skeleton_build_request() {
    let mut memory = ShortTermMemory::new(10);
    memory.append(Message {
        role: agentverse::memory::MessageRole::User,
        content: "Hello".to_string(),
    });

    let skeleton = agentverse_react::CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::new(Mutex::new(memory)),
        10,
    );

    let request = skeleton.build_request().unwrap();
    assert!(request.messages.iter().any(|m| m.content.contains("Hello")));
}

#[tokio::test]
async fn test_cycle_run_with_answer() {
    let mut skeleton = agentverse_react::CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        10,
    );

    let result = skeleton
        .run("test input".to_string(), |_s| async {
            Ok(CycleAction::Done {
                answer: "immediate".to_string(),
            })
        })
        .await;

    assert_eq!(result.unwrap().answer, "immediate");
}

#[tokio::test]
async fn test_cycle_run_max_iterations() {
    let mut skeleton = agentverse_react::CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec!["Thought: looping".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        2,
    );

    let result = skeleton
        .run("test".to_string(), |_s| async {
            Ok(CycleAction::Continue {
                thought: "looping".to_string(),
            })
        })
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        agentverse::AgentError::Model(agentverse::ModelError::Timeout(msg)) => {
            assert!(msg.contains("Max iterations"));
        }
        other => panic!("Expected Timeout error, got {:?}", other),
    }
}
