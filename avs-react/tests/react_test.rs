//! Tests for the agentverse-react crate.

use agentverse::SyncTool;
use agentverse::{
    GenerateRequest, GenerateResponse, Memory, Message, ModelProvider, PromptConfig,
    PromptRegistry, ShortTermMemory, UsageStats,
};
use agentverse_react::{parse::parse_response, CycleAction, CycleSkeleton, ReActStrategy};
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

// ─── ReActStrategy::run() tests ───────────────────────────────────────────────

#[tokio::test]
async fn test_react_strategy_direct_answer() {
    let model = MockModel {
        responses: vec!["Answer: 42".to_string()],
        index: AtomicUsize::new(0),
    };
    let mut strategy = ReActStrategy::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(model),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        10,
    );
    let result = strategy.run("What is 6*7?".to_string()).await.unwrap();
    assert_eq!(result.answer, "42");
}

#[tokio::test]
async fn test_react_strategy_thought_then_answer() {
    let model = MockModel {
        responses: vec![
            "Thought: let me think.".to_string(),
            "Answer: 42".to_string(),
        ],
        index: AtomicUsize::new(0),
    };
    let mut strategy = ReActStrategy::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(model),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        10,
    );
    let result = strategy.run("What is 6*7?".to_string()).await.unwrap();
    assert_eq!(result.answer, "42");
}

#[tokio::test]
async fn test_react_strategy_tool_call_then_answer() {
    let model = MockModel {
        responses: vec![
            "Thought: use tool.\nAction: echo\nAction Input: {}".to_string(),
            "Answer: done".to_string(),
        ],
        index: AtomicUsize::new(0),
    };
    let tool = MockTool::new("echo", "Echo back input");
    let mut strategy = ReActStrategy::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(model),
        vec![Box::new(tool)],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        10,
    );
    let result = strategy.run("use echo tool".to_string()).await.unwrap();
    assert_eq!(result.answer, "done");
}

#[tokio::test]
async fn test_react_strategy_max_iterations() {
    let model = MockModel {
        responses: vec!["Thought: still thinking.".to_string()],
        index: AtomicUsize::new(0),
    };
    let mut strategy = ReActStrategy::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(model),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        2,
    );
    let err = strategy.run("infinite loop".to_string()).await.unwrap_err();
    assert!(err.to_string().contains("Max iterations"));
}

#[tokio::test]
async fn test_react_strategy_empty_response_is_error() {
    let model = MockModel {
        responses: vec!["".to_string()],
        index: AtomicUsize::new(0),
    };
    let mut strategy = ReActStrategy::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(model),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        5,
    );
    let err = strategy.run("test".to_string()).await.unwrap_err();
    assert!(err.to_string().contains("Empty response"));
}

#[tokio::test]
async fn test_react_strategy_usage_accumulates() {
    let model = MockModel {
        responses: vec!["Answer: done".to_string()],
        index: AtomicUsize::new(0),
    };
    let mut strategy = ReActStrategy::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(model),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        5,
    );
    let result = strategy.run("test".to_string()).await.unwrap();
    // MockModel returns UsageStats::default() — total should remain zero
    assert_eq!(result.total_usage.input_tokens, 0);
}

// ─── CycleSkeleton preamble and guardrail tests ────────────────────────────────

#[test]
fn test_cycle_no_preamble_without_react_template() {
    // Default registry has react_template_loaded=false; prime is a no-op
    let memory = Arc::new(Mutex::new(ShortTermMemory::new(10)));
    let mut skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::clone(&memory),
        10,
    );
    skeleton.prime_react_preamble();
    assert!(!skeleton.is_react_primed());
    assert_eq!(memory.lock().unwrap().last_n(20).len(), 0);
}

#[tokio::test]
async fn test_cycle_preamble_inserted_when_react_template_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let react_path = dir.path().join("react.j2");
    std::fs::write(&react_path, "Tools: {{ tools }}\nUse ReAct format.").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(registry.has_react_template());

    let memory = Arc::new(Mutex::new(ShortTermMemory::new(10)));
    let mut skeleton = CycleSkeleton::new(
        Arc::new(registry),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![MockTool::new("test", "A test tool")]
            .into_iter()
            .map(|t| Box::new(t) as Box<dyn agentverse::SyncTool>)
            .collect(),
        Arc::clone(&memory),
        10,
    );

    skeleton.prime_react_preamble();
    assert!(skeleton.is_react_primed());

    let messages = memory.lock().unwrap().last_n(20);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].content.contains("Tools:"));
}

#[tokio::test]
async fn test_cycle_preamble_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("react.j2"), "Tools: {{ tools }}").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    let memory = Arc::new(Mutex::new(ShortTermMemory::new(10)));
    let mut skeleton = CycleSkeleton::new(
        Arc::new(registry),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::clone(&memory),
        10,
    );

    skeleton.prime_react_preamble();
    skeleton.prime_react_preamble(); // second call must be a no-op
    assert_eq!(memory.lock().unwrap().last_n(20).len(), 1);
}

#[test]
fn test_cycle_accumulate_usage() {
    use agentverse::UsageStats;
    let mut skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec![],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        5,
    );
    skeleton.accumulate_usage(UsageStats {
        input_tokens: 10,
        output_tokens: 5,
        ..Default::default()
    });
    skeleton.accumulate_usage(UsageStats {
        input_tokens: 20,
        output_tokens: 3,
        ..Default::default()
    });
    assert_eq!(skeleton.total_usage().input_tokens, 30);
    assert_eq!(skeleton.total_usage().output_tokens, 8);
}

#[test]
fn test_cycle_check_output_guardrail_clean() {
    let skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec![],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        5,
    );
    assert!(skeleton.check_output_guardrail("This is a clean response.").is_ok());
}

#[test]
fn test_cycle_build_request_with_guardrails_clean() {
    let skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec![],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        5,
    );
    assert!(skeleton.build_request_with_guardrails().is_ok());
}

#[test]
fn test_cycle_build_tools_str_with_parameters() {
    struct ParamTool;
    impl agentverse::SyncTool for ParamTool {
        fn name(&self) -> &str { "param_tool" }
        fn description(&self) -> &str { "Tool with params" }
        fn parameters(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query"},
                    "limit": {"type": "integer", "description": "Max results"}
                },
                "required": ["query"]
            })
        }
        fn execute(&self, _args: serde_json::Value) -> Result<serde_json::Value, agentverse::ToolError> {
            Ok(json!({}))
        }
    }

    let skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec![],
            index: AtomicUsize::new(0),
        }),
        vec![Box::new(ParamTool)],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        5,
    );
    let req = skeleton.build_request().unwrap();
    // System prompt should include parameter descriptions
    let system = req.system.unwrap();
    assert!(system.contains("query"));
    assert!(system.contains("required"));
}

#[tokio::test]
async fn test_cycle_run_with_tool_call() {
    let mut skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec!["Answer: done".to_string()],
            index: AtomicUsize::new(0),
        }),
        vec![Box::new(MockTool::new("echo", "echo"))],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        5,
    );
    let mut calls = 0usize;
    let result = skeleton
        .run("test".to_string(), |_s| {
            let action = if calls == 0 {
                calls += 1;
                CycleAction::ToolCall {
                    tool_name: "echo".to_string(),
                    args: json!({}),
                }
            } else {
                CycleAction::Done { answer: "done".to_string() }
            };
            async move { Ok(action) }
        })
        .await;
    assert_eq!(result.unwrap().answer, "done");
}

#[tokio::test]
async fn test_cycle_run_with_error_action() {
    let mut skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(MockModel {
            responses: vec![],
            index: AtomicUsize::new(0),
        }),
        vec![],
        Arc::new(Mutex::new(ShortTermMemory::new(10))),
        5,
    );
    let result = skeleton
        .run("test".to_string(), |_s| async {
            Ok(CycleAction::Error {
                message: "strategy error".to_string(),
            })
        })
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("strategy error"));
}
