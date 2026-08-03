use agentverse::{ConnectionManager, PromptRegistry};
use agentverse_subagent::*;
use agentverse_tools::ToolRegistry;
use httpmock::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

fn anthropic_answer_body(answer: &str) -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "text", "text": answer}],
        "usage": {
            "input_tokens": 50, "output_tokens": 10,
            "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0
        }
    })
}

fn make_executor(server_base_url: &str) -> SubAgentExecutor {
    let cm = Arc::new(ConnectionManager::anthropic(
        server_base_url,
        "claude-sonnet-4-6",
        "test-key",
    ));
    SubAgentExecutor::new(
        cm,
        Arc::clone(&ToolRegistry::new()),
        Arc::new(PromptRegistry::new()),
    )
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct LoopToolArgs {}

struct LoopTool;

#[async_trait::async_trait]
impl agentverse::Tool for LoopTool {
    type Args = LoopToolArgs;
    fn name(&self) -> &str {
        "loop_tool"
    }
    fn description(&self) -> &str {
        "a tool that always succeeds, used to force the subagent loop to keep iterating so budget limits can be tested"
    }
    async fn execute(&self, _args: LoopToolArgs) -> agentverse::ToolResult {
        Ok(serde_json::json!({"ok": true}))
    }
}

fn make_executor_with_loop_tool(server_base_url: &str) -> SubAgentExecutor {
    let cm = Arc::new(ConnectionManager::anthropic(
        server_base_url,
        "claude-sonnet-4-6",
        "test-key",
    ));
    let tools = ToolRegistry::new();
    tools.register(LoopTool);
    SubAgentExecutor::new(cm, tools, Arc::new(PromptRegistry::new()))
}

fn loop_tool_call_body(input_tokens: u32, output_tokens: u32) -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "tool_use", "id": "call_1", "name": "loop_tool", "input": {}}],
        "usage": {
            "input_tokens": input_tokens, "output_tokens": output_tokens,
            "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0
        }
    })
}

fn basic_spec() -> SubAgentSpec {
    SubAgentSpec {
        name: "test-agent".into(),
        objective: "Count to three".into(),
        system_prompt: None,
        model: None,
        allowed_tools: vec![],
        budget: Budget {
            max_steps: 5,
            max_tokens: 10_000,
            timeout: Duration::from_secs(10),
        },
    }
}

fn basic_ctx() -> SubAgentContext {
    SubAgentContext {
        resources: vec![],
        depth: 0,
    }
}

#[tokio::test]
async fn run_returns_answer_on_single_step() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(200)
            .json_body(anthropic_answer_body("The answer is 3."));
    });

    let executor = make_executor(&server.base_url());
    let result = executor.run(&basic_spec(), basic_ctx()).await.unwrap();

    assert_eq!(result.answer, "The answer is 3.");
    assert_eq!(result.steps, 1);
    assert!(result.usage.input_tokens > 0);
}

#[tokio::test]
async fn run_rejects_depth_exceeded() {
    let server = MockServer::start();
    let executor = make_executor(&server.base_url());
    let ctx = SubAgentContext {
        resources: vec![],
        depth: 1,
    }; // depth == max_depth (1)

    let err = executor.run(&basic_spec(), ctx).await.unwrap_err();
    assert!(matches!(err, SubAgentError::DepthExceeded));
}

#[tokio::test]
async fn run_enforces_step_budget() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST")
            .path("/v1/messages")
            .body_contains("loop_tool");
        then.status(200).json_body(loop_tool_call_body(10, 5));
    });

    let mut spec = basic_spec();
    spec.budget.max_steps = 2;
    spec.allowed_tools = vec!["loop_tool".to_string()];
    let executor = make_executor_with_loop_tool(&server.base_url());
    let err = executor.run(&spec, basic_ctx()).await.unwrap_err();
    assert!(matches!(
        err,
        SubAgentError::StepBudgetExceeded { steps: 2 }
    ));
}

#[tokio::test]
async fn run_enforces_token_budget() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(200).json_body(loop_tool_call_body(900, 200));
    });

    let mut spec = basic_spec();
    spec.budget.max_tokens = 500;
    spec.allowed_tools = vec!["loop_tool".to_string()];
    let executor = make_executor_with_loop_tool(&server.base_url());
    let err = executor.run(&spec, basic_ctx()).await.unwrap_err();
    assert!(matches!(err, SubAgentError::TokenBudgetExceeded { .. }));
}

#[tokio::test]
async fn run_injects_resources_into_message() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST")
            .path("/v1/messages")
            .body_contains("important content here")
            .body_contains("notes.md");
        then.status(200).json_body(anthropic_answer_body("done"));
    });

    let executor = make_executor(&server.base_url());
    let ctx = SubAgentContext {
        resources: vec![ResourceContent {
            label: "notes.md".into(),
            content: "important content here".into(),
        }],
        depth: 0,
    };

    let result = executor.run(&basic_spec(), ctx).await.unwrap();
    assert_eq!(result.answer, "done");
}

#[tokio::test]
async fn run_many_returns_all_results() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(200)
            .json_body(anthropic_answer_body("The answer is 3."));
    });

    let executor = make_executor(&server.base_url());
    let tasks = vec![
        (basic_spec(), basic_ctx()),
        (basic_spec(), basic_ctx()),
        (basic_spec(), basic_ctx()),
    ];
    let results = executor.run_many(tasks).await;

    assert_eq!(results.len(), 3);
    for result in results {
        let ok = result.unwrap();
        assert_eq!(ok.answer, "The answer is 3.");
    }
}

#[tokio::test]
async fn run_many_collects_all_results_on_failure() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(500);
    });

    let executor = make_executor(&server.base_url());
    let tasks = vec![
        (basic_spec(), basic_ctx()),
        (basic_spec(), basic_ctx()),
        (basic_spec(), basic_ctx()),
    ];
    let results = executor.run_many(tasks).await;

    assert_eq!(results.len(), 3);
    for result in results {
        assert!(matches!(result.unwrap_err(), SubAgentError::Llm(_)));
    }
}

#[tokio::test]
async fn spawn_returns_handle_and_result_is_available() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(200)
            .json_body(anthropic_answer_body("The answer is 3."));
    });

    let executor = make_executor(&server.base_url());
    let handle = executor.spawn(basic_spec(), basic_ctx());

    assert_ne!(handle.id, Uuid::nil());

    let result = handle.await_result().await.unwrap();
    assert_eq!(result.answer, "The answer is 3.");
}

#[test]
fn subagent_tool_name_and_schema() {
    use agentverse::ErasedTool;
    use std::sync::Arc;

    let server = MockServer::start();
    let executor = Arc::new(make_executor(&server.base_url()));
    let tool = SubAgentTool::new(executor, 0);
    let erased: &dyn ErasedTool = &tool;

    assert_eq!(erased.name(), "spawn_subagent");
    let schema = erased.schema();
    assert_eq!(schema["name"].as_str().unwrap(), "spawn_subagent");
    assert!(schema["input_schema"]["properties"].is_object());
}

#[tokio::test]
async fn subagent_tool_execute_calls_executor_and_returns_answer() {
    use agentverse::ErasedTool;
    use std::sync::Arc;

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(200)
            .json_body(anthropic_answer_body("tool answer"));
    });

    let executor = Arc::new(make_executor(&server.base_url()));
    let tool = SubAgentTool::new(executor, 0);
    let erased: &dyn ErasedTool = &tool;

    let args = serde_json::json!({
        "name": "my-worker",
        "objective": "do the thing",
        "allowed_tools": [],
        "resources": [],
        "max_steps": 3,
        "max_tokens": 5000,
        "timeout_secs": 10
    });

    let result = erased.execute_raw(args).await.unwrap();
    assert_eq!(result.as_str().unwrap(), "tool answer");
}
