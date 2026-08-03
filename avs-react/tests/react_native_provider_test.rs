//! End-to-end multi-tool-call coverage through the real provider wire
//! formats (not a hand-rolled RecordingProvider), proving two things per
//! provider: (1) two calls to the same tool are correctly correlated by
//! `id`, and (2) `ToolResult` blocks are restored to origin-call order
//! before being sent back, even though `ToolRegistry::execute_many` returns
//! them in completion order.

use agentverse::{
    ConnectionManager, LlmRunner, Message, MessageRole, PromptRegistry, RunStrategy,
    StrategyOutcome, Tool, ToolResult,
};
use agentverse_react::ReActStrategy;
use agentverse_tools::ToolRegistry;
use httpmock::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// httpmock 0.7's `.matches()` takes a plain `fn(&HttpMockRequest) -> bool`
// (type `MockMatcherFunction`), not a capturing closure, so the second-turn
// matcher below (which needs to stash the request body for later assertion)
// cannot close over a local `Arc<Mutex<..>>`. These module-level statics are
// the capture point instead: the matcher fns are still plain, non-capturing
// `fn` items (so they coerce to `MockMatcherFunction`), they just read/write
// global state rather than a closed-over variable.
static ANTHROPIC_SECOND_TURN_BODY: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static OPENAI_SECOND_TURN_BODY: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[derive(Debug, Deserialize, JsonSchema)]
struct DelayedArgs {}

/// A tool whose completion time is controlled by `delay_ms`, so a fast and a
/// slow instance registered under different names deterministically finish
/// out of the order they were called in — exactly what
/// `sort_results_by_call_order` exists to undo.
struct DelayedTool {
    tool_name: &'static str,
    delay_ms: u64,
}

#[async_trait::async_trait]
impl Tool for DelayedTool {
    type Args = DelayedArgs;

    fn name(&self) -> &str {
        self.tool_name
    }

    fn description(&self) -> &str {
        "test tool with an artificial delay, used to force out-of-order completion"
    }

    async fn execute(&self, _args: DelayedArgs) -> ToolResult {
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        Ok(json!({"tool": self.tool_name}))
    }
}

fn make_tools() -> Arc<ToolRegistry> {
    let tools = ToolRegistry::new();
    tools.register(DelayedTool {
        tool_name: "slow",
        delay_ms: 60,
    });
    tools.register(DelayedTool {
        tool_name: "fast",
        delay_ms: 0,
    });
    tools
}

#[tokio::test]
async fn anthropic_multi_tool_call_turn_correlates_and_orders_results() {
    let server = MockServer::start_async().await;

    fn is_first_turn(req: &HttpMockRequest) -> bool {
        let body = req
            .body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .unwrap_or_default();
        !body.contains("tool_result")
    }

    fn is_second_turn(req: &HttpMockRequest) -> bool {
        let body = req
            .body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .unwrap_or_default();
        if body.contains("tool_result") {
            *ANTHROPIC_SECOND_TURN_BODY
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap() = Some(body);
            true
        } else {
            false
        }
    }

    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/messages")
                .matches(is_first_turn);
            then.status(200).json_body(json!({
                "content": [
                    {"type": "tool_use", "id": "call_slow", "name": "slow", "input": {}},
                    {"type": "tool_use", "id": "call_fast", "name": "fast", "input": {}}
                ]
            }));
        })
        .await;

    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/messages")
                .matches(is_second_turn);
            then.status(200)
                .json_body(json!({"content": [{"type": "text", "text": "done"}]}));
        })
        .await;

    let runner = Arc::new(LlmRunner::new(Arc::new(ConnectionManager::anthropic(
        &server.base_url(),
        "claude-3-5-sonnet-20241022",
        "key",
    ))));
    let strategy = ReActStrategy::new(runner, Arc::new(PromptRegistry::new()), make_tools(), 5);

    let messages = vec![Message::text(MessageRole::User, "run both tools")];
    let result = strategy.run(messages).await;

    match result {
        Ok(StrategyOutcome::Done(answer)) => assert_eq!(answer, "done"),
        Ok(StrategyOutcome::Interrupted(_)) => panic!("expected Done, got Interrupted"),
        Err(e) => panic!("expected Done, got Err: {e}"),
    }

    let body = ANTHROPIC_SECOND_TURN_BODY
        .get()
        .and_then(|m| m.lock().unwrap().clone())
        .expect("second turn request must have been captured");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    let messages = parsed["messages"].as_array().unwrap();
    let tool_result_message = messages
        .iter()
        .find(|m| {
            m["content"]
                .as_array()
                .map(|blocks| blocks.iter().any(|b| b["type"] == "tool_result"))
                .unwrap_or(false)
        })
        .expect("a message with tool_result blocks must be present");
    let blocks = tool_result_message["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        blocks[0]["tool_use_id"], "call_slow",
        "results must be restored to call order (slow, fast), not completion order (fast, slow)"
    );
    assert_eq!(blocks[1]["tool_use_id"], "call_fast");
}

#[tokio::test]
async fn openai_compatible_multi_tool_call_turn_correlates_and_orders_results() {
    let server = MockServer::start_async().await;

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
            *OPENAI_SECOND_TURN_BODY
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap() = Some(body);
            true
        } else {
            false
        }
    }

    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .matches(is_first_turn);
            then.status(200).json_body(json!({
                "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_slow", "function": {"name": "slow", "arguments": "{}"}},
                    {"id": "call_fast", "function": {"name": "fast", "arguments": "{}"}}
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

    let runner = Arc::new(LlmRunner::new(Arc::new(ConnectionManager::openai(
        &server.base_url(),
        "test-model",
        "key",
    ))));
    let strategy = ReActStrategy::new(runner, Arc::new(PromptRegistry::new()), make_tools(), 5);

    let messages = vec![Message::text(MessageRole::User, "run both tools")];
    let result = strategy.run(messages).await;

    match result {
        Ok(StrategyOutcome::Done(answer)) => assert_eq!(answer, "done"),
        Ok(StrategyOutcome::Interrupted(_)) => panic!("expected Done, got Interrupted"),
        Err(e) => panic!("expected Done, got Err: {e}"),
    }

    let body = OPENAI_SECOND_TURN_BODY
        .get()
        .and_then(|m| m.lock().unwrap().clone())
        .expect("second turn request must have been captured");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    let messages = parsed["messages"].as_array().unwrap();
    let tool_messages: Vec<&serde_json::Value> =
        messages.iter().filter(|m| m["role"] == "tool").collect();
    assert_eq!(tool_messages.len(), 2);
    assert_eq!(
        tool_messages[0]["tool_call_id"], "call_slow",
        "tool messages must be restored to call order (slow, fast), not completion order"
    );
    assert_eq!(tool_messages[1]["tool_call_id"], "call_fast");
}
