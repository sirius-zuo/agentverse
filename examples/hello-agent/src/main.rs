// examples/hello-agent/src/main.rs
//
// Interactive Q&A agent: type a question, get an answer. Type "exit" or Ctrl+C to quit.
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-hello-agent

use agentverse::{OpenAICompatible, PromptConfig, PromptRegistry};
use agentverse_memory::SimpleMemory;
use agentverse_react::ReActStrategy;
use agentverse_logging as avs_logging;
use agentverse_tools::{Calculator, DateTimeTool, ToolRegistry};
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    tracing::info!(model = %model_name, base_url = %base_url, "Hello Agent");
    println!("Type your question and press Enter. Type \"exit\" or press Ctrl+C to quit.\n");

    let model = Arc::new(OpenAICompatible::new(&base_url, &model_name, &api_key));
    let registry = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );
    let memory = Arc::new(Mutex::new(SimpleMemory::new(50)));
    let mut tools = ToolRegistry::new();
    tools.register_with_category(Calculator, "math");
    tools.register_with_category(DateTimeTool, "utility");
    let mut agent = ReActStrategy::new(registry, model, tools, memory, 10);

    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    loop {
        print!("You: ");
        std::io::stdout().flush().ok();

        let input = match lines.next_line().await {
            Ok(Some(line)) => line,
            _ => {
                println!("\nGoodbye!");
                break;
            }
        };

        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("exit") {
            println!("Goodbye!");
            break;
        }

        match agent.run(input).await {
            Ok(result) => {
                println!("\nAgent: {}", result.answer);
                println!(
                    "[tokens] input={} output={} cache_read={} cache_write={}\n",
                    result.total_usage.input_tokens,
                    result.total_usage.output_tokens,
                    result.total_usage.cache_read_tokens,
                    result.total_usage.cache_write_tokens,
                );
            }
            Err(e) => eprintln!("Error: {}\n", e),
        }
    }
}
