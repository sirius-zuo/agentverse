use agentverse::memory::Message;
use agentverse::{
    AgentError as CoreAgentError, Config, LlmRunner, PromptRegistry, RunStrategy, StrategyOutcome,
    Tool, ToolResult,
};
use agentverse_agent::{agent::HitlConfig, Agent, AgentError, AgentOutput, SkillConfig, SkillMode};
use agentverse_hitl::{ApprovalDecision, HitlPolicy, InMemoryQueue};
use agentverse_router::{StrategyName, StrategyRouter};
use agentverse_session::SqliteSessionMemory;
use agentverse_tools::ToolRegistry;
use async_trait::async_trait;
use httpmock::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn chat_completion(content: &str) -> serde_json::Value {
    json!({
        "choices": [{"message": {"role": "assistant", "content": content}}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
    })
}

fn chat_completion_tool_call(id: &str, name: &str, arguments: &str) -> serde_json::Value {
    json!({
        "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [
            {"id": id, "function": {"name": name, "arguments": arguments}}
        ]}}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
    })
}

fn runner(base_url: &str) -> Arc<LlmRunner> {
    Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test",
                "sk-test",
                Some(base_url.to_string()),
            ),
            max_messages: 20,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    )
}

fn fixed_strategy(response: &str, calls: Arc<AtomicUsize>) -> Arc<dyn RunStrategy> {
    Arc::new(RecordingStrategy {
        response: response.to_string(),
        calls,
    })
}

struct RecordingStrategy {
    response: String,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl RunStrategy for RecordingStrategy {
    async fn run(&self, _messages: Vec<Message>) -> Result<StrategyOutcome, CoreAgentError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(StrategyOutcome::Done(self.response.clone()))
    }
}

#[derive(Deserialize, JsonSchema)]
struct EchoArgs {
    text: String,
}

struct EchoTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for EchoTool {
    type Args = EchoArgs;

    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo text"
    }

    async fn execute(&self, args: Self::Args) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({ "echo": args.text }))
    }
}

fn write_empty_tool_skill(root: &std::path::Path) {
    let package = root.join("system").join("no-tools");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("SKILL.md"),
        "---\nname: no-tools\ndescription: No tools.\nagentverse:\n  tools: []\n---\n\nDo not use tools.\n",
    )
    .unwrap();
}

async fn build_agent(
    strategy_runner: Arc<LlmRunner>,
    fixed: Arc<dyn RunStrategy>,
    router: Option<StrategyRouter>,
    hitl: bool,
) -> Arc<Agent> {
    let builder = Agent::builder(
        strategy_runner,
        ToolRegistry::new(),
        Arc::new(PromptRegistry::new()),
        Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap()),
        fixed,
    );
    let builder = match router {
        Some(router) => builder.with_strategy_router(router),
        None => builder,
    };
    if hitl {
        builder
            .with_hitl(HitlConfig {
                policy: HitlPolicy::new(),
                queue: Arc::new(InMemoryQueue::new()),
            })
            .build()
    } else {
        builder.build()
    }
}

#[tokio::test]
async fn strategy_router_selects_plan_strategy_on_every_invoke() {
    let router_server = MockServer::start_async().await;
    let route_mock = router_server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200)
                .json_body(chat_completion("plan_and_execute"));
        })
        .await;

    let strategy_server = MockServer::start_async().await;
    let synthesis_mock = strategy_server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_contains("Based on the executed plan");
            then.status(200)
                .json_body(chat_completion("selected plan strategy"));
        })
        .await;
    let plan_mock = strategy_server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_contains("You are a planning assistant");
            then.status(200).json_body(chat_completion(
                r#"{"description":"prove routing","steps":[{"id":1,"description":"selected plan branch","depends_on":[]}]}"#,
            ));
        })
        .await;

    let fixed_calls = Arc::new(AtomicUsize::new(0));
    let router = StrategyRouter::new(
        runner(&router_server.base_url()),
        vec![StrategyName::ReAct, StrategyName::PlanAndExecute],
    );
    let agent = build_agent(
        runner(&strategy_server.base_url()),
        fixed_strategy("fixed strategy", Arc::clone(&fixed_calls)),
        Some(router),
        false,
    )
    .await;
    let session_id = agent.create_session("alice").await.unwrap();

    for input in ["make a plan", "make another plan"] {
        let output = agent.invoke("alice", session_id, input).await.unwrap();
        assert!(matches!(
            output,
            AgentOutput::Done(ref text) if text == "selected plan strategy"
        ));
    }

    route_mock.assert_hits_async(2).await;
    plan_mock.assert_hits_async(2).await;
    synthesis_mock.assert_hits_async(2).await;
    assert_eq!(fixed_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn no_strategy_router_preserves_supplied_fixed_strategy() {
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = build_agent(
        runner("http://127.0.0.1:1"),
        fixed_strategy("fixed strategy", Arc::clone(&calls)),
        None,
        false,
    )
    .await;
    let session_id = agent.create_session("alice").await.unwrap();

    let output = agent
        .invoke("alice", session_id, "use the configured strategy")
        .await
        .unwrap();

    assert!(matches!(
        output,
        AgentOutput::Done(ref text) if text == "fixed strategy"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn strategy_router_fails_closed_when_plan_is_selected_with_hitl() {
    let router_server = MockServer::start_async().await;
    router_server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200)
                .json_body(chat_completion("plan_and_execute"));
        })
        .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let router = StrategyRouter::new(
        runner(&router_server.base_url()),
        vec![StrategyName::ReAct, StrategyName::PlanAndExecute],
    );
    let agent = build_agent(
        runner("http://127.0.0.1:1"),
        fixed_strategy("must not run", Arc::clone(&calls)),
        Some(router),
        true,
    )
    .await;
    let session_id = agent.create_session("alice").await.unwrap();

    let error = agent
        .invoke("alice", session_id, "make a plan")
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        AgentError::RoutedStrategyDoesNotSupportHitl {
            strategy: StrategyName::PlanAndExecute
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn routed_strategy_cannot_execute_tools_excluded_by_active_tool_names() {
    let router_server = MockServer::start_async().await;
    router_server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).json_body(chat_completion("react"));
        })
        .await;

    let strategy_server = MockServer::start_async().await;
    strategy_server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_contains("\"role\":\"tool\"");
            then.status(200).json_body(chat_completion("tool ran"));
        })
        .await;
    strategy_server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_contains("attempt hidden tool");
            then.status(200).json_body(chat_completion_tool_call(
                "call_1",
                "echo",
                "{\"text\":\"secret\"}",
            ));
        })
        .await;

    let tool_calls = Arc::new(AtomicUsize::new(0));
    let tools = ToolRegistry::new();
    tools.register(EchoTool {
        calls: Arc::clone(&tool_calls),
    });
    let skills_dir = tempfile::tempdir().unwrap();
    write_empty_tool_skill(skills_dir.path());
    let skills = SkillConfig::load(skills_dir.path(), SkillMode::Open).unwrap();
    let fixed_calls = Arc::new(AtomicUsize::new(0));
    let agent = Agent::builder(
        runner(&strategy_server.base_url()),
        tools,
        Arc::new(PromptRegistry::new()),
        Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap()),
        fixed_strategy("fixed strategy", Arc::clone(&fixed_calls)),
    )
    .with_skills(skills)
    .with_strategy_router(StrategyRouter::new(
        runner(&router_server.base_url()),
        vec![StrategyName::ReAct],
    ))
    .build();
    let session_id = agent
        .create_session_with_skill("alice", "no-tools")
        .await
        .unwrap();

    // The excluded tool is not found in the restricted registry, so the call
    // fails internally and is fed back to the model as an observation (same
    // recoverable-error handling as the HITL twin below), rather than
    // aborting the whole invocation. The security property under test is
    // that the real tool body never runs.
    let output = agent
        .invoke("alice", session_id, "attempt hidden tool")
        .await
        .unwrap();

    assert!(matches!(
        output,
        AgentOutput::Done(ref text) if text == "tool ran"
    ));
    assert_eq!(tool_calls.load(Ordering::SeqCst), 0);
    assert_eq!(fixed_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn routed_strategy_cannot_execute_inactive_tool_after_hitl_approval() {
    let router_server = MockServer::start_async().await;
    let route_mock = router_server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).json_body(chat_completion("react"));
        })
        .await;

    let strategy_server = MockServer::start_async().await;
    strategy_server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_contains("Tool: echo");
            then.status(200)
                .json_body(chat_completion("approval handled"));
        })
        .await;
    strategy_server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_contains("attempt approved hidden tool");
            then.status(200).json_body(chat_completion_tool_call(
                "call_1",
                "echo",
                "{\"text\":\"secret\"}",
            ));
        })
        .await;

    let tool_calls = Arc::new(AtomicUsize::new(0));
    let tools = ToolRegistry::new();
    tools.register(EchoTool {
        calls: Arc::clone(&tool_calls),
    });
    let skills_dir = tempfile::tempdir().unwrap();
    write_empty_tool_skill(skills_dir.path());
    let skills = SkillConfig::load(skills_dir.path(), SkillMode::Open).unwrap();
    let agent = Agent::builder(
        runner(&strategy_server.base_url()),
        tools,
        Arc::new(PromptRegistry::new()),
        Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap()),
        fixed_strategy("fixed strategy", Arc::new(AtomicUsize::new(0))),
    )
    .with_skills(skills)
    .with_strategy_router(StrategyRouter::new(
        runner(&router_server.base_url()),
        vec![StrategyName::ReAct],
    ))
    .with_hitl(HitlConfig {
        policy: HitlPolicy {
            global_tool_blocklist: HashSet::from(["echo".to_string()]),
            ..HitlPolicy::default()
        },
        queue: Arc::new(InMemoryQueue::new()),
    })
    .build();
    let session_id = agent
        .create_session_with_skill("alice", "no-tools")
        .await
        .unwrap();

    let approval_id = match agent
        .invoke("alice", session_id, "attempt approved hidden tool")
        .await
        .unwrap()
    {
        AgentOutput::Interrupted { approval_id, .. } => approval_id,
        AgentOutput::Done(text) => panic!("expected HITL interrupt, got {text}"),
    };
    let output = agent
        .resume("alice", session_id, approval_id, ApprovalDecision::Approved)
        .await
        .unwrap();

    assert!(matches!(
        output,
        AgentOutput::Done(ref text) if text == "approval handled"
    ));
    route_mock.assert_hits_async(1).await;
    assert_eq!(tool_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn routed_react_strategy_is_preserved_across_hitl_resume() {
    let router_server = MockServer::start_async().await;
    let route_mock = router_server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).json_body(chat_completion("react"));
        })
        .await;

    let strategy_server = MockServer::start_async().await;
    strategy_server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_contains("Tool: echo");
            then.status(200)
                .json_body(chat_completion("routed react resumed"));
        })
        .await;
    strategy_server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_contains("call echo");
            then.status(200).json_body(chat_completion_tool_call(
                "call_1",
                "echo",
                "{\"text\":\"hi\"}",
            ));
        })
        .await;

    let tools = ToolRegistry::new();
    let tool_calls = Arc::new(AtomicUsize::new(0));
    tools.register(EchoTool {
        calls: Arc::clone(&tool_calls),
    });
    let fixed_calls = Arc::new(AtomicUsize::new(0));
    let router = StrategyRouter::new(
        runner(&router_server.base_url()),
        vec![StrategyName::ReAct, StrategyName::PlanAndExecute],
    );
    let agent = Agent::builder(
        runner(&strategy_server.base_url()),
        tools,
        Arc::new(PromptRegistry::new()),
        Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap()),
        fixed_strategy("fixed strategy", Arc::clone(&fixed_calls)),
    )
    .with_strategy_router(router)
    .with_hitl(HitlConfig {
        policy: HitlPolicy {
            global_tool_blocklist: HashSet::from(["echo".to_string()]),
            ..HitlPolicy::default()
        },
        queue: Arc::new(InMemoryQueue::new()),
    })
    .build();
    let session_id = agent.create_session("alice").await.unwrap();

    let approval_id = match agent
        .invoke("alice", session_id, "call echo")
        .await
        .unwrap()
    {
        AgentOutput::Interrupted { approval_id, .. } => approval_id,
        AgentOutput::Done(text) => panic!("expected HITL interrupt, got {text}"),
    };
    let output = agent
        .resume("alice", session_id, approval_id, ApprovalDecision::Approved)
        .await
        .unwrap();

    assert!(matches!(
        output,
        AgentOutput::Done(ref text) if text == "routed react resumed"
    ));
    route_mock.assert_hits_async(1).await;
    assert_eq!(fixed_calls.load(Ordering::SeqCst), 0);
}
