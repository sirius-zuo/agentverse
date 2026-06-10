use agentverse::{ConnectionManager, PromptRegistry};
use agentverse_subagent::*;
use agentverse_tools::ToolRegistry;
use httpmock::prelude::*;
use std::sync::Arc;
use std::time::Duration;

fn anthropic_answer_body(answer: &str) -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "text", "text": format!("Answer: {answer}")}],
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
        when.method("POST").path("/v1/messages");
        then.status(200).json_body(serde_json::json!({
            "content": [{"type": "text", "text": "Thought: still thinking..."}],
            "usage": {"input_tokens": 10, "output_tokens": 5,
                      "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
        }));
    });

    let mut spec = basic_spec();
    spec.budget.max_steps = 2;
    let executor = make_executor(&server.base_url());
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
        then.status(200).json_body(serde_json::json!({
            "content": [{"type": "text", "text": "Thought: thinking..."}],
            "usage": {"input_tokens": 900, "output_tokens": 200,
                      "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
        }));
    });

    let mut spec = basic_spec();
    spec.budget.max_tokens = 500;
    let executor = make_executor(&server.base_url());
    let err = executor.run(&spec, basic_ctx()).await.unwrap_err();
    assert!(matches!(err, SubAgentError::TokenBudgetExceeded { .. }));
}

#[tokio::test]
async fn run_injects_resources_into_message() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
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
