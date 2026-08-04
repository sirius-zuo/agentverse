//! Tests for the agentverse-react crate: `CycleSkeleton` behavior. See also
//! `react_strategy_test.rs` for `ReActStrategy`/HITL tests, split out to stay
//! under the workspace's file-size cap.

use agentverse::{
    Config, LlmRunner, Message, MessageRole, PromptConfig, PromptRegistry, Tool, ToolResult,
};
use agentverse_react::CycleSkeleton;
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
    );
    CycleSkeleton::new(
        runner,
        Arc::new(PromptRegistry::new()),
        ToolRegistry::new(),
        5,
    )
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
    let msgs = vec![Message::text(MessageRole::User, "hi")];
    let buf = s.prepare_buffer(msgs);
    assert_eq!(buf.len(), 1);
}

#[test]
fn test_cycle_preamble_inserted_when_react_template_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let react_path = dir.path().join("react.j2");
    std::fs::write(&react_path, "Follow the ReAct pattern.\n{{ examples }}").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(registry.has_react_template());

    let runner = Arc::new(
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
    );
    let tools = ToolRegistry::new();
    tools.register(MockTool::new("test", "A test tool"));

    let s = CycleSkeleton::new(runner, Arc::new(registry), tools, 10);

    let msgs = vec![Message::text(MessageRole::User, "hello")];
    let buf = s.prepare_buffer(msgs);
    assert_eq!(buf.len(), 2);
    assert!(buf[0].as_text().contains("Follow the ReAct pattern."));
}

#[test]
fn test_check_output_guardrail_clean() {
    let s = make_skeleton();
    assert!(s
        .check_output_guardrail("This is a clean response.")
        .is_ok());
}
