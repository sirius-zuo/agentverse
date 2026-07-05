use agentverse::{Config, LlmRunner, PromptRegistry, RunStrategy, Tool, ToolResult};
use agentverse_eval::judge::{build_judge_connection, run_judge, Verdict};
use agentverse_eval::recording::{load_recording, register_agent_turns, register_judge_turn};
use agentverse_react::ReActStrategy;
use agentverse_tools::ToolRegistry;
use httpmock::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
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
