//! Tests for the agentverse-react crate.

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, RunStrategy, Tool, ToolResult};
use agentverse_react::{parse::parse_response, CycleAction, CycleSkeleton, ReActStrategy};
use agentverse_tools::ToolRegistry;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

// ─── Mock tool ────────────────────────────────────────────────────────────────

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

fn make_skeleton() -> CycleSkeleton {
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::OpenAI {
                model_name: "test".to_string(),
                api_key: "sk-test".to_string(),
                base_url: Some("http://127.0.0.1:1/v1".to_string()),
            },
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    CycleSkeleton::new(
        runner,
        Arc::new(PromptRegistry::new()),
        ToolRegistry::new(),
        5,
    )
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

// ─── CycleSkeleton tests ──────────────────────────────────────────────────────

#[test]
fn test_cycle_skeleton_tool_count_zero() {
    let s = make_skeleton();
    // find_tools is auto-registered by ToolRegistry::new()
    assert!(s.tool_count() >= 1);
}

#[test]
fn test_cycle_skeleton_max_iterations() {
    let s = make_skeleton();
    assert_eq!(s.max_iterations(), 5);
}

#[test]
fn test_cycle_skeleton_tool_count_nonzero() {
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::OpenAI {
                model_name: "test".to_string(),
                api_key: "sk-test".to_string(),
                base_url: Some("http://127.0.0.1:1/v1".to_string()),
            },
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    tools.register(MockTool::new("echo", "Echo tool"));
    let initial = tools.len();
    let s = CycleSkeleton::new(runner, Arc::new(PromptRegistry::new()), tools, 10);
    assert_eq!(s.tool_count(), initial);
}

#[test]
fn test_cycle_skeleton_prepare_buffer_no_preamble() {
    let s = make_skeleton();
    let msgs = vec![agentverse::Message {
        role: agentverse::MessageRole::User,
        content: "hi".to_string(),
    }];
    let buf = s.prepare_buffer(msgs);
    assert_eq!(buf.len(), 1);
}

#[test]
fn test_cycle_preamble_inserted_when_react_template_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let react_path = dir.path().join("react.j2");
    std::fs::write(&react_path, "Tools: {{ tools }}\nUse ReAct format.").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(registry.has_react_template());

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::OpenAI {
                model_name: "test".to_string(),
                api_key: "sk-test".to_string(),
                base_url: Some("http://127.0.0.1:1/v1".to_string()),
            },
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    tools.register(MockTool::new("test", "A test tool"));

    let s = CycleSkeleton::new(runner, Arc::new(registry), tools, 10);

    let msgs = vec![agentverse::Message {
        role: agentverse::MessageRole::User,
        content: "hello".to_string(),
    }];
    let buf = s.prepare_buffer(msgs);
    assert_eq!(buf.len(), 2);
    assert!(buf[0].content.contains("Tools:"));
}

#[test]
fn test_check_output_guardrail_clean() {
    let s = make_skeleton();
    assert!(s
        .check_output_guardrail("This is a clean response.")
        .is_ok());
}

#[tokio::test]
async fn test_cycle_skeleton_execute_tool() {
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::OpenAI {
                model_name: "test".to_string(),
                api_key: "sk-test".to_string(),
                base_url: Some("http://127.0.0.1:1/v1".to_string()),
            },
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    tools.register(MockTool::new("echo", "Echo back input"));
    let s = CycleSkeleton::new(runner, Arc::new(PromptRegistry::new()), tools, 10);

    let result = s
        .execute_tool("echo", json!({"text": "hello"}))
        .await
        .unwrap();
    assert!(result.contains("Executed echo"));
}

#[tokio::test]
async fn test_cycle_skeleton_execute_tool_not_found() {
    let s = make_skeleton();
    let result = s.execute_tool("missing_tool", json!({})).await;
    assert!(result.is_err());
}

// ─── ReActStrategy tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn react_run_returns_error_on_bad_port() {
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::OpenAI {
                model_name: "test".to_string(),
                api_key: "sk-test".to_string(),
                base_url: Some("http://127.0.0.1:1/v1".to_string()),
            },
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let strategy = ReActStrategy::new(
        runner,
        Arc::new(PromptRegistry::new()),
        ToolRegistry::new(),
        3,
    );

    let messages = vec![agentverse::Message {
        role: agentverse::MessageRole::User,
        content: "What is 2+2?".to_string(),
    }];

    let result = strategy.run(messages).await;
    assert!(result.is_err(), "Expected error when LLM is unreachable");
}

// ─── run_hitl interrupt tests ──────────────────────────────────────────────

#[tokio::test]
async fn run_hitl_returns_interrupted_with_typed_history_and_pending_calls() {
    use agentverse::hitl::{ApprovalId, HitlHook};
    use agentverse::StrategyOutcome;
    use httpmock::prelude::*;

    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).json_body(json!({
                "choices": [{"message": {"role": "assistant", "content": "Thought: need to call the tool.\nAction: echo\nAction Input: {\"text\": \"hi\"}"}}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
            }));
        })
        .await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::OpenAI {
                model_name: "test".to_string(),
                api_key: "sk-test".to_string(),
                base_url: Some(server.base_url()),
            },
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );

    let tools = ToolRegistry::new();
    tools.register(MockTool::new("echo", "Echo tool"));

    let strategy = ReActStrategy::new(runner, Arc::new(PromptRegistry::new()), tools, 5);

    struct AlwaysBlockHook;
    #[async_trait::async_trait]
    impl HitlHook for AlwaysBlockHook {
        async fn check_tool(
            &self,
            tool_name: &str,
            _args: &serde_json::Value,
        ) -> Option<(ApprovalId, String)> {
            Some((
                uuid::Uuid::new_v4(),
                format!("{{\"tool\":\"{}\"}}", tool_name),
            ))
        }
    }
    let hook: Arc<dyn HitlHook> = Arc::new(AlwaysBlockHook);

    let messages = vec![agentverse::Message {
        role: agentverse::MessageRole::User,
        content: "please call echo".to_string(),
    }];

    let result = strategy
        .run_hitl(messages, &["echo".to_string()], hook)
        .await;

    match result {
        Ok(StrategyOutcome::Interrupted(interrupt)) => {
            assert_eq!(
                interrupt.pending_calls.len(),
                1,
                "pending_calls must contain the intercepted call"
            );
            assert_eq!(interrupt.pending_calls[0].name, "echo");
            assert_eq!(interrupt.pending_calls[0].args, json!({"text": "hi"}));
            assert!(!interrupt.history.is_empty(), "history must be non-empty");
            assert_eq!(interrupt.history[0].content, "please call echo");
            assert_eq!(interrupt.active_tool_names, vec!["echo".to_string()]);
        }
        Ok(StrategyOutcome::Done(text)) => panic!("expected Interrupted, got Done({text})"),
        Err(e) => panic!("expected Interrupted, got Err: {e}"),
    }
}
