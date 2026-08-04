//! Tests for the agentverse-react crate: `ReActStrategy` behavior and HITL
//! interrupts. Split out of `react_test.rs` (which keeps the `CycleSkeleton`
//! tests) to stay under the workspace's file-size cap; the `MockTool` helper
//! below is intentionally duplicated from `react_test.rs` rather than shared,
//! since each file is its own integration-test binary.

use agentverse::{
    Config, ConnectionManager, GenerateRequest, GenerateResponse, LlmRunner, Message, MessageRole,
    ModelError, ModelProvider, PromptRegistry, RunStrategy, Tool, ToolResult,
};
use agentverse_react::ReActStrategy;
use agentverse_tools::ToolRegistry;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex, OnceLock};

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

// ─── ReActStrategy tests ──────────────────────────────────────────────────────

struct RecordingProvider {
    request: Arc<Mutex<Option<GenerateRequest>>>,
}

impl ModelProvider for RecordingProvider {
    fn name(&self) -> &str {
        "recording"
    }

    fn build_request(
        &self,
        _model: &str,
        request: GenerateRequest,
    ) -> Result<serde_json::Value, ModelError> {
        *self.request.lock().unwrap() = Some(request);
        Err(ModelError::InvalidResponse(
            "stop after recording".to_string(),
        ))
    }

    fn parse_response(&self, _body: &str) -> Result<GenerateResponse, ModelError> {
        Err(ModelError::InvalidResponse("not used".to_string()))
    }

    fn request_headers(&self, _api_key: &str) -> reqwest::header::HeaderMap {
        reqwest::header::HeaderMap::new()
    }

    fn endpoint_path(&self, _model: &str) -> String {
        "/recording".to_string()
    }
}

fn recording_strategy() -> (ReActStrategy, Arc<Mutex<Option<GenerateRequest>>>) {
    let request = Arc::new(Mutex::new(None));
    let runner = Arc::new(LlmRunner::new(Arc::new(ConnectionManager::new(
        RecordingProvider {
            request: Arc::clone(&request),
        },
        "http://unused",
        "key",
        "model",
    ))));
    let tools = ToolRegistry::new();
    tools.register(MockTool::new("echo", "Echo tool"));

    (
        ReActStrategy::new(runner, Arc::new(PromptRegistry::new()), tools, 1),
        request,
    )
}

fn user_message() -> Vec<Message> {
    vec![Message::text(MessageRole::User, "hello")]
}

#[tokio::test]
async fn run_with_active_tools_forwards_non_empty_definitions() {
    let (strategy, request) = recording_strategy();

    let result = strategy
        .run_with_active_tools(user_message(), &["echo".to_string()])
        .await;

    assert!(result.is_err(), "recording provider stops the model call");
    let request = request.lock().unwrap().take().unwrap();
    let definitions = request.tools.expect("active tool definitions must be sent");
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "echo");
    assert_eq!(definitions[0].description, "Echo tool");
}

#[tokio::test]
async fn run_with_empty_or_unknown_active_tools_sends_none() {
    for active_tool_names in [Vec::new(), vec!["missing".to_string()]] {
        let (strategy, request) = recording_strategy();

        let result = strategy
            .run_with_active_tools(user_message(), &active_tool_names)
            .await;

        assert!(result.is_err(), "recording provider stops the model call");
        let request = request.lock().unwrap().take().unwrap();
        assert!(
            request.tools.is_none(),
            "empty resolved definitions must remain None"
        );
    }
}

#[tokio::test]
async fn run_hitl_forwards_non_empty_active_tool_definitions() {
    use agentverse::hitl::{ApprovalId, HitlHook};

    struct AllowAllHook;

    #[async_trait::async_trait]
    impl HitlHook for AllowAllHook {
        async fn check_tool(
            &self,
            _tool_name: &str,
            _args: &serde_json::Value,
        ) -> Option<(ApprovalId, String)> {
            None
        }
    }

    let (strategy, request) = recording_strategy();
    let result = strategy
        .run_hitl(
            user_message(),
            &["echo".to_string()],
            Arc::new(AllowAllHook),
        )
        .await;

    assert!(result.is_err(), "recording provider stops the model call");
    let request = request.lock().unwrap().take().unwrap();
    let definitions = request.tools.expect("active tool definitions must be sent");
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "echo");
}

#[tokio::test]
async fn react_run_returns_error_on_bad_port() {
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
    let strategy = ReActStrategy::new(
        runner,
        Arc::new(PromptRegistry::new()),
        ToolRegistry::new(),
        3,
    );

    let messages = vec![Message::text(MessageRole::User, "What is 2+2?")];

    let result = strategy.run(messages).await;
    assert!(result.is_err(), "Expected error when LLM is unreachable");
}

#[tokio::test]
async fn react_run_text_only_first_response_is_immediate_done() {
    use httpmock::prelude::*;

    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).json_body(json!({
                "choices": [{"message": {"role": "assistant", "content": "The answer is 4.", "tool_calls": []}}]
            }));
        })
        .await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test".to_string(),
                "sk-test".to_string(),
                Some(server.base_url()),
            ),
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
        5,
    );

    let result = strategy.run(user_message()).await;

    match result {
        Ok(agentverse::StrategyOutcome::Done(answer)) => assert_eq!(answer, "The answer is 4."),
        Ok(agentverse::StrategyOutcome::Interrupted(_)) => panic!("expected Done, got Interrupted"),
        Err(e) => panic!("expected Done, got Err: {e}"),
    }
}

#[tokio::test]
async fn react_run_recovers_from_tool_not_found_error() {
    use httpmock::prelude::*;

    fn chat_completion_text(content: &str) -> serde_json::Value {
        json!({
            "choices": [{"message": {"role": "assistant", "content": content, "tool_calls": []}}]
        })
    }

    fn chat_completion_tool_call(id: &str, name: &str, arguments: &str) -> serde_json::Value {
        json!({
            "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [
                {"id": id, "function": {"name": name, "arguments": arguments}}
            ]}}]
        })
    }

    fn is_first_turn(req: &HttpMockRequest) -> bool {
        let body = req
            .body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .unwrap_or_default();
        !body.contains("\"role\":\"tool\"")
    }

    fn is_second_turn(req: &HttpMockRequest) -> bool {
        let body = req
            .body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .unwrap_or_default();
        body.contains("\"role\":\"tool\"")
    }

    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .matches(is_first_turn);
            then.status(200)
                .json_body(chat_completion_tool_call("call_1", "missing_tool", "{}"));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .matches(is_second_turn);
            then.status(200).json_body(chat_completion_text("done"));
        })
        .await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test".to_string(),
                "sk-test".to_string(),
                Some(server.base_url()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );

    // "missing_tool" is intentionally not registered, so the first call fails
    // with ToolError::NotFound — this must come back as an `is_error` tool
    // result the model can react to, not a hard error that aborts the loop.
    let tools = ToolRegistry::new();
    let strategy = ReActStrategy::new(runner, Arc::new(PromptRegistry::new()), tools, 5);

    let result = strategy.run(user_message()).await;

    match result {
        Ok(agentverse::StrategyOutcome::Done(answer)) => assert_eq!(answer, "done"),
        Ok(agentverse::StrategyOutcome::Interrupted(_)) => panic!("expected Done, got Interrupted"),
        Err(e) => panic!("expected recovery via retry, got Err: {e}"),
    }
}

// httpmock 0.7's `.matches()` takes a plain `fn(&HttpMockRequest) -> bool`
// (type `MockMatcherFunction`), not a capturing closure, so the second-turn
// matcher below (which needs to stash the request body for later assertion)
// cannot close over a local `Arc<Mutex<..>>`. This module-level static is
// the capture point instead — see `avs-react/tests/react_native_provider_test.rs`
// for the same pattern.
static SAME_TOOL_SECOND_TURN_BODY: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[tokio::test]
async fn react_run_dispatches_two_calls_to_the_same_tool_in_one_turn() {
    use httpmock::prelude::*;

    fn is_first_turn(req: &HttpMockRequest) -> bool {
        let body = req
            .body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .unwrap_or_default();
        !body.contains("\"role\":\"tool\"")
    }

    fn is_second_turn(req: &HttpMockRequest) -> bool {
        let body = req
            .body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .unwrap_or_default();
        if body.contains("\"role\":\"tool\"") {
            *SAME_TOOL_SECOND_TURN_BODY
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap() = Some(body);
            true
        } else {
            false
        }
    }

    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .matches(is_first_turn);
            then.status(200).json_body(json!({
                "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_a", "function": {"name": "echo", "arguments": "{}"}},
                    {"id": "call_b", "function": {"name": "echo", "arguments": "{}"}}
                ]}}]
            }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .matches(is_second_turn);
            then.status(200).json_body(json!({
                "choices": [{"message": {"role": "assistant", "content": "done", "tool_calls": []}}]
            }));
        })
        .await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test".to_string(),
                "sk-test".to_string(),
                Some(server.base_url()),
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
    let strategy = ReActStrategy::new(runner, Arc::new(PromptRegistry::new()), tools, 5);

    let result = strategy.run(user_message()).await;

    match result {
        Ok(agentverse::StrategyOutcome::Done(answer)) => assert_eq!(answer, "done"),
        Ok(agentverse::StrategyOutcome::Interrupted(_)) => panic!("expected Done, got Interrupted"),
        Err(e) => panic!("expected Done, got Err: {e}"),
    }

    let body = SAME_TOOL_SECOND_TURN_BODY
        .get()
        .and_then(|m| m.lock().unwrap().clone())
        .expect("second turn request must have been captured");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    let messages = parsed["messages"].as_array().unwrap();
    let tool_call_ids: Vec<&str> = messages
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["tool_call_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        tool_call_ids,
        vec!["call_a", "call_b"],
        "two calls to the same tool name must still be correlated and ordered by id"
    );
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
                "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "function": {"name": "echo", "arguments": "{\"text\": \"hi\"}"}}
                ]}}]
            }));
        })
        .await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test".to_string(),
                "sk-test".to_string(),
                Some(server.base_url()),
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

    let messages = vec![Message::text(MessageRole::User, "please call echo")];

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
            assert_eq!(interrupt.pending_calls[0].id, "call_1");
            assert_eq!(interrupt.pending_calls[0].name, "echo");
            assert_eq!(interrupt.pending_calls[0].args, json!({"text": "hi"}));
            assert!(!interrupt.history.is_empty(), "history must be non-empty");
            assert_eq!(interrupt.history[0].as_text(), "please call echo");
            assert_eq!(interrupt.active_tool_names, vec!["echo".to_string()]);
        }
        Ok(StrategyOutcome::Done(text)) => panic!("expected Interrupted, got Done({text})"),
        Err(e) => panic!("expected Interrupted, got Err: {e}"),
    }
}
