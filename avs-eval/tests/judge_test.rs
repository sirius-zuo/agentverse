use agentverse::{Config, LlmRunner, PromptRegistry, RunStrategy, Tool, ToolResult};
use agentverse_agent::agent::HitlConfig;
use agentverse_agent::{Agent, AgentOutput};
use agentverse_eval::judge::{build_judge_connection, run_judge, Verdict};
use agentverse_eval::recording::{load_recording, register_agent_turns, register_judge_turn};
use agentverse_hitl::{ApprovalDecision, HitlPolicy, InMemoryQueue, InterruptKind};
use agentverse_plan::PlanStrategy;
use agentverse_react::ReActStrategy;
use agentverse_session::{SessionMemory, SqliteSessionMemory};
use agentverse_skill::{SkillConfig, SkillMode};
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::ToolRegistry;
use httpmock::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::sync::Arc;

#[derive(Debug, Deserialize, JsonSchema)]
struct EchoArgs {
    text: String,
}

struct EchoTool;

#[async_trait::async_trait]
impl Tool for EchoTool {
    type Args = EchoArgs;
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "Echoes text back"
    }
    async fn execute(&self, args: EchoArgs) -> ToolResult {
        Ok(json!({"echoed": args.text}))
    }
}

#[tokio::test]
async fn react_tool_call_passes_judge() {
    let recording = load_recording("react_tool_call");

    let agent_server = MockServer::start_async().await;
    register_agent_turns(&agent_server, &recording).await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test-model".to_string(),
                "sk-test".to_string(),
                Some(agent_server.base_url()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    tools.register(EchoTool);
    let strategy = ReActStrategy::new(runner, Arc::new(PromptRegistry::new()), tools, 5);

    let messages = vec![agentverse::Message {
        role: agentverse::MessageRole::User,
        content: "please echo hello".to_string(),
    }];
    let outcome = strategy.run(messages).await.unwrap();
    let agent_output = match outcome {
        agentverse::StrategyOutcome::Done(answer) => answer,
        agentverse::StrategyOutcome::Interrupted(_) => panic!("expected Done, got Interrupted"),
    };

    let judge_server = MockServer::start_async().await;
    register_judge_turn(&judge_server, &recording).await;
    let judge_connection =
        build_judge_connection(&judge_server.base_url(), "judge-model", "test-key").unwrap();
    let verdict = run_judge(
        &judge_connection,
        "the answer must use the tool's actual returned value and not fabricate one",
        &agent_output,
    )
    .await
    .unwrap();

    assert_eq!(
        verdict.verdict,
        Verdict::Pass,
        "judge failed: {}",
        verdict.reasoning
    );
}

#[tokio::test]
async fn plan_multi_step_passes_judge() {
    let recording = load_recording("plan_multi_step");

    let agent_server = MockServer::start_async().await;
    register_agent_turns(&agent_server, &recording).await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test-model".to_string(),
                "sk-test".to_string(),
                Some(agent_server.base_url()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    tools.register(EchoTool);
    let strategy = PlanStrategy::new(runner, Arc::new(PromptRegistry::new()), tools, 10);

    let messages = vec![agentverse::Message {
        role: agentverse::MessageRole::User,
        content: "what is the answer".to_string(),
    }];
    let outcome = strategy.run(messages).await.unwrap();
    let agent_output = match outcome {
        agentverse::StrategyOutcome::Done(answer) => answer,
        agentverse::StrategyOutcome::Interrupted(_) => panic!("expected Done, got Interrupted"),
    };

    let judge_server = MockServer::start_async().await;
    register_judge_turn(&judge_server, &recording).await;
    let judge_connection =
        build_judge_connection(&judge_server.base_url(), "judge-model", "test-key").unwrap();
    let verdict = run_judge(
        &judge_connection,
        "the final answer must address the original request, not just the last sub-step",
        &agent_output,
    )
    .await
    .unwrap();

    assert_eq!(
        verdict.verdict,
        Verdict::Pass,
        "judge failed: {}",
        verdict.reasoning
    );
}

fn write_skill(dir: &std::path::Path, name: &str, description: &str, instructions: &str) {
    let pkg = dir.join("system").join(name);
    fs::create_dir_all(&pkg).unwrap();
    let content = format!("---\nname: {name}\ndescription: {description}\n---\n\n{instructions}\n");
    fs::write(pkg.join("SKILL.md"), content).unwrap();
}

#[tokio::test]
async fn skill_routed_response_passes_judge() {
    let recording = load_recording("skill_routed_response");

    let dir = tempfile::tempdir().unwrap();
    write_skill(
        dir.path(),
        "code-review",
        "Review code for bugs and style issues.",
        "You are an expert code reviewer. Stay strictly within code review — do not answer unrelated questions.",
    );
    let skills = SkillConfig::load(dir.path(), SkillMode::Open).unwrap();

    let agent_server = MockServer::start_async().await;
    register_agent_turns(&agent_server, &recording).await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test-model".to_string(),
                "sk-test".to_string(),
                Some(agent_server.base_url()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    let prompts = Arc::new(PromptRegistry::new());
    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        3,
    );
    let session_memory = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
    let agent = Agent::builder(
        runner,
        tools,
        prompts,
        Arc::clone(&session_memory) as Arc<dyn SessionMemory>,
        strategy,
    )
    .with_skills(skills)
    .build();

    let session_id = agent.create_session("alice").await.unwrap();
    let output = agent
        .invoke("alice", session_id, "please review my code for bugs")
        .await
        .unwrap();
    let agent_output = match output {
        AgentOutput::Done(text) => text,
        other => panic!("expected Done, got {other:?}"),
    };

    let ctx_json = session_memory.get_skill_context(session_id).await.unwrap();
    assert!(
        ctx_json.is_some(),
        "skill should have activated via routing"
    );

    let judge_server = MockServer::start_async().await;
    register_judge_turn(&judge_server, &recording).await;
    let judge_connection =
        build_judge_connection(&judge_server.base_url(), "judge-model", "test-key").unwrap();
    let verdict = run_judge(
        &judge_connection,
        "the response must stay within the code-review skill's declared scope",
        &agent_output,
    )
    .await
    .unwrap();

    assert_eq!(
        verdict.verdict,
        Verdict::Pass,
        "judge failed: {}",
        verdict.reasoning
    );
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExecCommandArgs {
    cmd: String,
}
struct ExecCommandTool;
#[async_trait::async_trait]
impl Tool for ExecCommandTool {
    type Args = ExecCommandArgs;
    fn name(&self) -> &str {
        "exec_command"
    }
    fn description(&self) -> &str {
        "Runs a shell command"
    }
    async fn execute(&self, args: ExecCommandArgs) -> ToolResult {
        Ok(json!({"output": format!("ran: {}", args.cmd)}))
    }
}

#[tokio::test]
async fn hitl_interrupt_resume_passes_judge() {
    let recording = load_recording("hitl_interrupt_resume");

    let agent_server = MockServer::start_async().await;
    register_agent_turns(&agent_server, &recording).await;

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test-model".to_string(),
                "sk-test".to_string(),
                Some(agent_server.base_url()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    tools.register(ExecCommandTool);
    let prompts = Arc::new(PromptRegistry::new());
    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        5,
    );
    let session_memory = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
    let agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
        .with_hitl(HitlConfig {
            policy: HitlPolicy::new(),
            queue: Arc::new(InMemoryQueue::new()),
        })
        .build();

    let session_id = agent.create_session("alice").await.unwrap();
    let first_output = agent
        .invoke("alice", session_id, "please run ls")
        .await
        .unwrap();
    let approval_id = match first_output {
        AgentOutput::Interrupted { approval_id, kind } => {
            assert!(matches!(kind, InterruptKind::ToolApproval { .. }));
            approval_id
        }
        other => panic!("expected Interrupted, got {other:?}"),
    };

    let resumed_output = agent
        .resume("alice", session_id, approval_id, ApprovalDecision::Approved)
        .await
        .unwrap();
    let agent_output = match resumed_output {
        AgentOutput::Done(text) => text,
        other => panic!("expected Done after resume, got {other:?}"),
    };

    let judge_server = MockServer::start_async().await;
    register_judge_turn(&judge_server, &recording).await;
    let judge_connection =
        build_judge_connection(&judge_server.base_url(), "judge-model", "test-key").unwrap();
    let verdict = run_judge(
        &judge_connection,
        "the resumed response must correctly reflect the approved tool's actual result",
        &agent_output,
    )
    .await
    .unwrap();

    assert_eq!(
        verdict.verdict,
        Verdict::Pass,
        "judge failed: {}",
        verdict.reasoning
    );
}
