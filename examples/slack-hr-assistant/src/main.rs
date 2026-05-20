// examples/slack-hr-assistant/src/main.rs
//
// Slack HR assistant using the Integration architecture.
// Receives Slack messages via Events API, processes with PlanStrategy, replies to Slack.
//
// Run:
//   SLACK_BOT_TOKEN=xoxb-... \
//   SLACK_SIGNING_SECRET=... \
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=your-model \
//   cargo run -p example-slack-hr-assistant
use agentverse::{
    GenerateRequest, GenerateResponse, ModelError, ModelProvider, OpenAICompatible, PromptConfig,
    PromptRegistry,
};
use agentverse_integration::{Integration, SlackConnector, StrategyInvoker};
use agentverse_memory::SimpleMemory;
use agentverse_plan::PlanStrategy;
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::Mutex;

struct LoggingModel<M>(M);

#[async_trait::async_trait]
impl<M: ModelProvider + Send + Sync> ModelProvider for LoggingModel<M> {
    fn name(&self) -> &str {
        self.0.name()
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        println!("┌─ generate() ──────────────────────────────────────────");
        if let Some(sys) = &request.system {
            println!("│ [system]\n│ {}", sys.replace('\n', "\n│ "));
        }
        for msg in &request.messages {
            let role = format!("{:?}", msg.role).to_lowercase();
            println!("│ [{role}]\n│ {}", msg.content.replace('\n', "\n│ "));
        }
        if let Some(tools) = &request.tools {
            println!("│ [tools] {} registered", tools.len());
        }
        println!("└───────────────────────────────────────────────────────");
        self.0.generate(request).await
    }
}

#[tokio::main]
async fn main() {
    let bot_token = std::env::var("SLACK_BOT_TOKEN").expect("SLACK_BOT_TOKEN not set");
    let signing_secret =
        std::env::var("SLACK_SIGNING_SECRET").expect("SLACK_SIGNING_SECRET not set");
    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".into());
    let model_name = std::env::var("MODEL_NAME").unwrap_or_else(|_| "gpt-4".into());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();

    let model = Arc::new(LoggingModel(OpenAICompatible::new(
        &base_url,
        &model_name,
        &api_key,
    )));
    let registry = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );
    let memory = Arc::new(Mutex::new(SimpleMemory::new(50)));
    let tools = ToolRegistry::new();

    let strategy = PlanStrategy::new(model, registry, tools, memory, 10);
    let invoker = StrategyInvoker::new(strategy);

    // Wrap in Arc so the same connector can serve as both input and output.
    let slack = Arc::new(SlackConnector::new(&bot_token, &signing_secret, 3000));

    let integration = Integration::new(
        Box::new(Arc::clone(&slack)), // input: receive Slack messages
        Box::new(invoker),
        vec![Box::new(Arc::clone(&slack))], // output: reply to Slack
    );

    println!("HR assistant listening on port 3000…");
    integration.run().await.expect("integration failed");
}
