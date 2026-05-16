// examples/anthropic-react/src/main.rs
//
// ReAct agent using AnthropicProvider with prompt caching.
//
// A custom system prompt is registered that is long enough (>1024 tokens) to
// cross Anthropic's minimum cacheable size.  It is tagged with
// cache_control: ephemeral on every request.  The first call writes it to the
// cache (cache_write_tokens > 0); subsequent iterations within the same run
// read it back at ~10 % of normal input token cost (cache_read_tokens > 0).
// The [tokens] line at the end shows the cumulative split.
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   cargo run -p example-anthropic-react

use agentverse::{AnthropicProvider, PromptConfig, PromptRegistry, ShortTermMemory};
use agentverse_react::ReActStrategy;
use agentverse_tools::Calculator;
use std::sync::{Arc, Mutex};

/// A substantial system prompt that intentionally exceeds Anthropic's 1 024-token
/// cache minimum so that prompt-caching is actually exercised across iterations.
///
/// In production this would be your domain-specific instructions, company
/// policies, tool documentation, few-shot examples, etc.  Here we include a
/// detailed description of the ReAct pattern and the calculator tool so the
/// content is meaningful rather than filler.
const SYSTEM_PROMPT: &str = "\
You are a precise mathematical reasoning assistant that solves problems \
step-by-step using the ReAct (Reasoning + Acting) pattern.

## Your role
You break every problem down into the smallest possible arithmetic steps, \
solve each one with the calculator tool, and only state a final answer once \
every intermediate result has been verified by an actual tool call.  You never \
perform arithmetic mentally — every addition, subtraction, multiplication, and \
division must go through the calculator.

## ReAct format
Every response you produce must follow this exact structure:

    Thought: <your reasoning about what to do next>
    Action: calculator
    Action Input: {\"operation\": \"<op>\", \"a\": <number>, \"b\": <number>}

When you have computed the final answer and all sub-expressions are resolved:

    Thought: <brief summary of what you computed>
    Answer: <final numeric result>

Never skip the Thought line.  Never combine multiple operations into a single \
Action Input — one tool call per step.

## Calculator tool
Name: calculator
Operations: add, subtract, multiply, divide
Parameters (all required):
  - operation (string): one of \"add\", \"subtract\", \"multiply\", \"divide\"
  - a (number): left operand
  - b (number): right operand
Returns: {\"result\": <number>}

Example — computing 3 * (4 + 5):

    Thought: I need 4 + 5 first.
    Action: calculator
    Action Input: {\"operation\": \"add\", \"a\": 4, \"b\": 5}

    Tool: calculator
    Result: {\"result\": 9}

    Thought: Now multiply 3 by 9.
    Action: calculator
    Action Input: {\"operation\": \"multiply\", \"a\": 3, \"b\": 9}

    Tool: calculator
    Result: {\"result\": 27}

    Thought: The result is 27.
    Answer: 27

## Rules
1. One operation per Action Input — never nest or combine.
2. Use the result from each tool call as input to the next where needed.
3. Do not approximate.  Compute exactly.
4. If division produces a non-integer, report the decimal result from the tool.
5. Show your full reasoning chain — every intermediate step must appear.
6. State the final Answer only after all required tool calls are complete.
7. Never invent tool results — only use what the tool actually returns.

## Why step-by-step matters
Breaking compound expressions into atomic operations makes each step \
independently verifiable and prevents accumulated rounding or sign errors.  \
Even for expressions that look simple, use the tool for every sub-expression.

## About prompt caching
This system prompt is intentionally substantial so that it crosses Anthropic's \
1 024-token minimum for ephemeral prompt caching.  On the first ReAct \
iteration the entire system prompt is written to Anthropic's server-side cache \
(visible as cache_write_tokens in the usage response).  On every subsequent \
iteration within the same request sequence the cached copy is served back at \
roughly one-tenth the token cost (visible as cache_read_tokens).  This makes \
long, stable system prompts significantly cheaper for multi-turn agents.\
";

#[tokio::main]
async fn main() {
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("ANTHROPIC_API_KEY is not set");
        std::process::exit(1);
    });
    let model_name = std::env::var("ANTHROPIC_MODEL")
        .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());

    println!("Anthropic ReAct Agent — model: {}", model_name);
    println!("Tool: Calculator");
    println!("Prompt caching: enabled (system prompt + penultimate message)");
    println!();

    let model = Arc::new(AnthropicProvider::new(
        "https://api.anthropic.com",
        &model_name,
        &api_key,
    ));

    // Register the custom system prompt so build_request() renders it instead
    // of DEFAULT_SYSTEM_TEMPLATE.
    let registry = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            system_prompt: Some(SYSTEM_PROMPT.to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );

    let memory = Arc::new(Mutex::new(ShortTermMemory::new(50)));
    let tools: Vec<Box<dyn agentverse::SyncTool>> = vec![Box::new(Calculator)];
    let mut agent = ReActStrategy::new(registry, model, tools, memory, 15);

    // Multi-step arithmetic that requires three sequential tool calls:
    //   step 1: 137 * 48  = 6576
    //   step 2: 256 / 4   = 64
    //   step 3: 6576 + 64 = 6640
    //   step 4: 6640 - 19 = 6621
    // Each iteration re-sends the system prompt; after the first write it is
    // served from cache, so cache_read_tokens grows with each loop iteration.
    let question = "What is (137 * 48) + (256 / 4) - 19?";
    println!("> {}", question);

    match agent.run(question.to_string()).await {
        Ok(result) => {
            println!("\nAgent: {}", result.answer);
            println!(
                "\n[tokens] input={} output={} cache_write={} cache_read={}",
                result.total_usage.input_tokens,
                result.total_usage.output_tokens,
                result.total_usage.cache_write_tokens,
                result.total_usage.cache_read_tokens,
            );
            if result.total_usage.cache_read_tokens > 0 {
                println!(
                    "[cache]  {} tokens served from cache across loop iterations",
                    result.total_usage.cache_read_tokens
                );
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
