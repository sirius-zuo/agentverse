// examples/web-search-agent/src/main.rs
//
// Plan-and-Execute agent: search a topic on DuckDuckGo, fetch the top N pages,
// and produce a summarized answer with sources.
//
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-web-search-agent -- "rust async programming" 3

use agentverse::{
    GenerateRequest, GenerateResponse, ModelError, ModelProvider, OpenAICompatible, PromptConfig,
    PromptRegistry,
};
use agentverse_memory::SimpleMemory;
use agentverse_plan::PlanStrategy;
use agentverse_tools::{ToolRegistry, WebSearch};
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
        println!("└───────────────────────────────────────────────────────");
        self.0.generate(request).await
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <topic> <n>", args[0]);
        eprintln!("  topic  Search topic (quote multi-word topics)");
        eprintln!("  n      Number of results to fetch and summarize (1-10)");
        std::process::exit(1);
    }
    let topic = &args[1];
    let n: u64 = match args[2].parse() {
        Ok(v) if v >= 1 => v,
        _ => {
            eprintln!("Error: <n> must be a positive integer (1-10)");
            std::process::exit(1);
        }
    };

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    println!(
        "Web Search Agent — topic: {:?}, n: {}, model: {} @ {}",
        topic, n, model_name, base_url
    );

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
        .expect("failed to load prompt config"),
    );
    let memory = Arc::new(Mutex::new(SimpleMemory::new(50)));
    let mut tools = ToolRegistry::new();
    tools.register(WebSearch);
    let mut agent = PlanStrategy::new(model, registry, tools, memory, 5);

    let question = format!(
        "Search for '{}' and summarize the top {} results.",
        topic, n
    );
    println!("> {}", question);

    match agent.run(question).await {
        Ok(answer) => println!("\nAgent: {}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
