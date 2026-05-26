# AgentVerse Developer Guide

Complete guide for developing, testing, and deploying agents with AgentVerse.

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Development Setup](#development-setup)
- [Creating a Custom Agent](#creating-a-custom-agent)
- [Multi-LLM Provider Configuration](#multi-llm-provider-configuration)
- [Writing Tools](#writing-tools)
- [Prompt Engineering](#prompt-engineering)
- [Testing Strategies](#testing-strategies)
- [Deploying Agents](#deploying-agents)
- [Adding Long-Term Memory](#adding-long-term-memory)
- [Integrating External Systems](#integrating-external-systems)
- [Debugging & Observability](#debugging--observability)

---

## Architecture Overview

AgentVerse is a modular Rust framework organized as a Cargo workspace:

```
AgentVerse/
├── avs-core/              # LlmRunner, Config, ProviderConfig, PromptRegistry, Memory + Tool traits
├── avs-agent/             # Agent: single LLM access point; optional HTTP sidecar (feature = "http")
├── avs-strategy/          # build() factory + StrategyKind enum; re-exports all strategies
├── avs-session/           # Session model, SessionManager, SessionMemory trait, SqliteSessionMemory
├── avs-logging/           # avs_logging::init() (RUST_LOG / LOG_FORMAT)
├── avs-react/             # ReAct strategy loop
├── avs-plan/              # Plan-and-Execute + Hierarchical strategies
├── avs-router/            # Dynamic strategy routing
├── avs-tools/             # Built-in tools (Calculator, DateTime, FileSearch, HttpClient, WebSearch, ShellTool)
├── avs-mcp/               # MCP client for external tool servers
├── avs-guardrails/        # Security: prompt injection, output filtering, rate limiting
├── avs-integration/       # IntegrationRuntime with Slack, console connectors
├── avs-memory/            # Layer-1 working buffer (SimpleMemory, AgentMemory) and LongTermBackend trait
├── avs-memory-lancedb/    # LanceDB Layer-3 LongtermMemory backend
├── avs-memory-pgvector/   # pgvector Layer-3 LongtermMemory backend + PostgresSessionMemory
└── examples/
    ├── hello-agent/        # Interactive REPL (ReAct + Calculator + DateTime)
    ├── react-calculator/   # Multi-step ReAct with Calculator
    ├── web-search-agent/   # Plan-and-Execute with WebSearch
    ├── anthropic-react/    # Anthropic Claude with prompt caching
    ├── code-review-agent/  # Hierarchical planning with FileSearch + ShellTool
    ├── slack-hr-assistant/ # IntegrationRuntime Slack/console bot
    └── http-agent/         # Agent with enable_http_server=true
```

### Key Concepts

| Concept | Crate | Description |
|---------|-------|-------------|
| **Agent** | `agentverse-agent` | Single LLM access point — composes `LlmRunner`, strategy, `SessionManager`, and memory layers |
| **StrategyKind** | `agentverse-strategy` | Enum selecting the orchestration loop; `build()` constructs an `Arc<dyn RunStrategy>` |
| **LlmRunner** | `agentverse` | Renders prompts and calls model providers |
| **Config** | `agentverse` | Provider settings (model name, API key, base URL) |
| **PromptRegistry** | `agentverse` | Template engine (Minijinja) + example storage |
| **Tool** | `agentverse` | `AsyncTool` trait — the single interface for all agent tools |
| **SessionMemory** | `agentverse-session` | Layer-2 durable conversation transcript; `SessionManager` wraps it with ownership checks |
| **LongtermMemory** | `agentverse` | Layer-3 cross-session knowledge store; opt-in via `Agent::new(..., Some(store))` |
| **RunStrategy** | `agentverse` | Trait implemented by all strategies; pure `Vec<Message> → String`, no memory coupling |

---

## Development Setup

### Prerequisites

- **Rust 1.75+** (MSRV) — `rustup install stable`
- **Cargo** — bundled with Rust
- **llama.cpp** (optional, for local development) — see [Local LLM Development](#local-llm-development)
- **Docker** (optional, for pgvector testing)

### Workspace Commands

```bash
# Check the entire workspace
cargo check --workspace

# Run all tests
cargo test --workspace

# Run clippy with warnings as errors
cargo clippy --all -- -D warnings

# Format all code
cargo fmt --all
```

### Local LLM Development

For local development without API costs, run llama.cpp as an OpenAI-compatible server:

```bash
git clone https://github.com/ggerganov/llama.cpp.git
cd llama.cpp
cmake -B build && cmake --build build --config Release -n

# Start the OpenAI-compatible server
./build/bin/llama-server -m models/your-model.gguf --host 127.0.0.1 --port 9090
```

Then set your environment variables:

```bash
export MODEL_BASE_URL=http://127.0.0.1:9090/v1
export MODEL_NAME=your-model
# MODEL_API_KEY is optional when MODEL_BASE_URL points to a local endpoint
```

---

## Creating a Custom Agent

All agents follow the same pattern: construct an `Agent` with a chosen strategy via `agentverse_strategy::build`.

### Cargo.toml

```toml
[dependencies]
agentverse = { path = "path/to/avs-core" }
agentverse-agent = { path = "path/to/avs-agent" }
agentverse-strategy = { path = "path/to/avs-strategy" }
agentverse-session = { path = "path/to/avs-session" }
agentverse-logging = { path = "path/to/avs-logging" }
agentverse-tools = { path = "path/to/avs-tools" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

### Option 1: Console Agent (ReAct)

```rust
use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{Calculator, DateTimeTool, ToolRegistry};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let base_url = std::env::var("MODEL_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name = std::env::var("MODEL_NAME")
        .unwrap_or_else(|_| "my-model".to_string());

    let runner = Arc::new(LlmRunner::from_config(Config {
        provider: ProviderConfig::OpenAI {
            model_name,
            api_key,
            base_url: Some(base_url),
        },
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    }).expect("runner"));

    let mut tool_registry = ToolRegistry::new();
    tool_registry.register_with_category(Calculator, "math");
    tool_registry.register_with_category(DateTimeTool, "utility");
    let tools = Arc::new(tool_registry);

    let prompts = Arc::new(PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
        ..Default::default()
    }).expect("prompts"));

    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        10,
    );

    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:").await.expect("session memory")
    );

    // enable_http_server=false: console only; None = no long-term memory
    let agent = Agent::new(runner, tools, prompts, session_memory, strategy, false, None);

    // Stateless invoke (no session history)
    match agent.invoke_stateless("What is 6 * 7?").await {
        Ok(answer) => println!("Agent: {}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

### Option 2: Agent with Session History

```rust
// Create a session for a user
let session_id = agent.create_session("alice").await?;

// Invoke with session — history is loaded, persisted, and returned
let reply = agent.invoke("alice", session_id, "What did I just say?").await?;
```

### Option 3: HTTP Agent

Add `features = ["http"]` to `agentverse-agent` in Cargo.toml and pass `enable_http_server=true`:

```toml
agentverse-agent = { path = "path/to/avs-agent", features = ["http"] }
```

```rust
// enable_http_server=true: spawns HTTP server as a background task
// reads HOST (default 0.0.0.0) and PORT (default 3000) from env
let _agent = Agent::new(runner, tools, prompts, session_memory, strategy, true, None);

// Keep the process alive
tokio::signal::ctrl_c().await.unwrap();
```

### Option 4: Anthropic Claude

```rust
let runner = Arc::new(LlmRunner::from_config(Config {
    provider: ProviderConfig::Anthropic {
        model_name: "claude-sonnet-4-6".to_string(),
        api_key: std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY"),
    },
    max_messages: 50,
    tools: vec![],
    prompts_dir: None,
    system_prompt: None,
}).expect("runner"));
```

### Choosing a Strategy

| `StrategyKind` | Best for |
|---|---|
| `React` | Tool-using agents, Q&A, step-by-step reasoning |
| `Plan` | Multi-step tasks that benefit from upfront planning |
| `Hierarchical` | Complex tasks decomposed into independent sub-goals |

---

## Multi-LLM Provider Configuration

### ProviderConfig Enum

| Variant | Fields | Use Case |
|---------|--------|----------|
| `OpenAI` | `model_name`, `api_key`, `base_url` | OpenAI API or any OpenAI-compatible endpoint (llama.cpp, Ollama, vLLM, etc.). `api_key` is optional when `base_url` is set. |
| `Anthropic` | `model_name`, `api_key` | Claude models via Anthropic API |
| `Gemini` | `model_name`, `api_key` | Google Gemini models |

### Configuration Examples

**Local OpenAI-compatible endpoint:**
```rust
ProviderConfig::OpenAI {
    model_name: "my-model".to_string(),
    api_key: String::new(),            // empty is fine for local endpoints
    base_url: Some("http://127.0.0.1:9090/v1".to_string()),
}
```

**OpenAI:**
```rust
ProviderConfig::OpenAI {
    model_name: "gpt-4o".to_string(),
    api_key: std::env::var("OPENAI_API_KEY").unwrap(),
    base_url: None,                    // uses OpenAI default
}
```

**Anthropic:**
```rust
ProviderConfig::Anthropic {
    model_name: "claude-sonnet-4-6".to_string(),
    api_key: std::env::var("ANTHROPIC_API_KEY").unwrap(),
}
```

**Gemini:**
```rust
ProviderConfig::Gemini {
    model_name: "gemini-pro".to_string(),
    api_key: std::env::var("GEMINI_API_KEY").unwrap(),
}
```

---

## Writing Tools

All tools implement `AsyncTool` — a single, consistent interface.

### Implementing AsyncTool

```rust
use agentverse::{AsyncTool, ToolError, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct WeatherTool;

#[async_trait]
impl AsyncTool for WeatherTool {
    fn name(&self) -> &str { "weather" }

    fn description(&self) -> &str { "Get current weather for a city" }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "city": { "type": "string", "description": "City name" }
            },
            "required": ["city"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let city = args["city"].as_str()
            .ok_or_else(|| ToolError::Execution("missing city".into()))?;
        Ok(json!({ "weather": format!("Sunny in {city}") }))
    }
}
```

### ToolRegistry

```rust
use agentverse_tools::{ToolRegistry, Calculator, DateTimeTool, ShellTool, WebSearch};
use std::time::Duration;

let mut registry = ToolRegistry::new();
registry.register_with_category(Calculator, "math");
registry.register_with_category(DateTimeTool, "utility");
registry.register(WebSearch);
registry.register_with_category(WeatherTool, "network");

// Shell tool — sandboxed subprocess execution
registry.register_with_category(
    ShellTool::new(
        "./workspace",
        Duration::from_secs(30),
        vec!["sudo".into(), "rm".into()],
    ),
    "shell",
);
```

### ShellTool

`ShellTool` runs shell commands via `tokio::process::Command`.

```rust
let tool = ShellTool::new(
    "./project",
    Duration::from_secs(30),
    vec!["sudo".into(), "curl".into()],
);
```

The agent calls it with `{ "command": "cargo test -p my-crate" }`. Response is plain text: stdout + `[stderr: ...]` if non-empty + `[exit code: N]` if non-zero.

**Security note:** `workdir` sets the initial directory but does not restrict filesystem access. Pair with a blocked-command list and OS-level isolation for production use.

---

## Prompt Engineering

AgentVerse uses a **three-layer prompt system** designed to maximize LLM prompt cache reuse.

### Template Roles

| Layer | File | Contains | Cache behaviour |
|---|---|---|---|
| System | `system.j2` | Agent identity + rules | Cached in the system block — paid once per session |
| Preamble | `react.j2` | Tool descriptions + format instructions + few-shot examples | Inserted as `messages[0]`; captured by the penultimate-message cache breakpoint |
| Conversation | *(memory)* | Thought / Action / Tool Result / Answer exchanges | Volatile |

### Directory Layout

**ReAct strategy:**
```
prompts/
  system.j2              # Identity + rules
  react.j2               # Tools + format + {% if examples %}...{% endif %}
  react_examples.toml    # Few-shot examples (name: "react_examples")
```

**Plan-and-Execute:**
```
prompts/
  system.j2
  plan_and_execute.j2    # → "strategies.plan_and_execute"
  plan_examples.toml
```

**Hierarchical:**
```
prompts/
  system.j2
  hierarchical.j2        # → "strategies.hierarchical.decompose"
  hierarchical_examples.toml
```

### Template Files (.j2)

`react.j2` example:

```jinja2
Available tools:
{{ tools }}

Always respond in this exact format:

    Thought: <your reasoning>
    Action: <tool_name>
    Action Input: <json arguments>

When you have the final answer:

    Thought: <brief summary>
    Answer: <final result>

{% if examples %}
Examples:
{% for example in examples %}
User: {{ example.input }}
Assistant: {{ example.output }}
{% endfor %}
{% endif %}
```

### Example Files (.toml)

```toml
# prompts/react_examples.toml
[[example]]
input = "What is 6 * 7?"
output = "Thought: I need to multiply.\nAction: calculator\nAction Input: {\"operation\": \"multiply\", \"a\": 6, \"b\": 7}"
```

---

## Testing Strategies

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_tool() {
        let tool = MyTool;
        assert_eq!(tool.name(), "my_tool");
        assert!(tool.description().contains("useful"));
    }
}
```

### Integration Tests

```rust
// tests/integration_test.rs
use agentverse::{Config, LlmRunner, ProviderConfig};

#[test]
fn test_runner_creation() {
    let runner = LlmRunner::from_config(Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: "test-key".to_string(),
            base_url: None,
        },
        max_messages: 10,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    });
    assert!(runner.is_ok());
}
```

### Running Tests

```bash
cargo test --workspace
cargo test -p agentverse-agent
cargo test --workspace -- --nocapture
```

---

## Deploying Agents

### Option 1: Standalone Console Binary

```bash
cargo build --release -p example-hello-agent
MODEL_BASE_URL=http://127.0.0.1:9090/v1 MODEL_NAME=my-model \
  ./target/release/example-hello-agent
```

### Option 2: HTTP Agent Binary

```bash
cargo build --release -p example-http-agent
ANTHROPIC_API_KEY=sk-ant-... HOST=0.0.0.0 PORT=3000 \
  ./target/release/example-http-agent
```

Or build your own binary with `enable_http_server=true` and `features = ["http"]` — see [Option 3 in Creating a Custom Agent](#option-3-http-agent).

### Option 3: Docker

```dockerfile
FROM rust:1.75-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p example-http-agent

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/example-http-agent .
EXPOSE 3000
CMD ["./example-http-agent"]
```

```bash
docker build -t agentverse-http .
docker run -p 3000:3000 \
  -e ANTHROPIC_API_KEY=sk-ant-... \
  -e HOST=0.0.0.0 \
  -e PORT=3000 \
  agentverse-http
```

---

## Adding Long-Term Memory

Layer-3 `LongtermMemory` is opt-in. Pass `Some(store)` as the last argument to `Agent::new`; pass `None` to disable it entirely.

```rust
use agentverse::memory::LongtermMemory;
use std::sync::Arc;

// Any type implementing LongtermMemory works here
let longterm: Arc<dyn LongtermMemory> = Arc::new(MyLongtermStore::new().await?);

let agent = Agent::new(runner, tools, prompts, session_memory, strategy, false, Some(longterm));
```

On each `invoke` call the agent:
1. Retrieves the top-k scored memories (`score = α·recency + β·importance + γ·relevance`) and injects them into the system prompt.
2. Asynchronously writes the completed turn as a `LongtermRecord` (fire-and-forget, off the latency path).

Background workers (`ConsolidationWorker`, `CleanupWorker` in `avs-agent`) handle batch consolidation and retention-window cleanup independently of the per-turn write.

### SQLite database location

`SqliteSessionMemory` (Layer 2) uses a file URL like `"sqlite:agent.db"`. The file is created in the working directory where `cargo run` is invoked (typically the repo root). Most examples use `"sqlite::memory:"` — an in-process database that does not write to disk and is lost when the process exits.

To inspect data at runtime:
```bash
sqlite3 agent.db "SELECT session_id, role, content FROM messages ORDER BY sequence_num;"
# or enable sqlx trace logging for bound parameters:
RUST_LOG=sqlx=trace cargo run -p example-http-agent
```

---

## Integrating External Systems

### IntegrationRuntime

`IntegrationRuntime` is the unified connector host. Your agent provides a handler closure; the runtime calls it for each incoming event.

```rust
use agentverse_integration::{Event, IntegrationRuntime};
use std::sync::Arc;

let agent = Arc::new(Agent::new(/* ... */));
let runtime = IntegrationRuntime::from_config("agent.toml").await?;

runtime
    .run(move |event: Event| {
        let agent = Arc::clone(&agent);
        async move {
            let answer = agent
                .invoke_stateless(&event.text)
                .await
                .map_err(|e| agentverse::AgentError::Memory(e.to_string()))?;
            Ok::<Event, agentverse::AgentError>(Event { text: answer, ..event })
        }
    })
    .await?;
```

**`agent.toml` example (Slack connector):**
```toml
[integration]
input = "slack"
outputs = ["slack"]

[connector.slack]
port = 3000
bot_token_env = "SLACK_BOT_TOKEN"
signing_secret_env = "SLACK_SIGNING_SECRET"
```

If no config file is found, `IntegrationRuntime::from_config` falls back to `ConsoleConnector` — useful for local development.

---

## Debugging & Observability

### Logging Setup

All examples call `avs_logging::init()` at startup. This reads two environment variables:

| Variable | Effect |
|----------|--------|
| `RUST_LOG` | Log level filter — e.g. `info`, `debug`, `agentverse_react=trace` |
| `LOG_FORMAT` | Set to `json` for structured JSON output; omit for human-readable text |

```bash
RUST_LOG=debug cargo run -p example-hello-agent
LOG_FORMAT=json RUST_LOG=info cargo run -p example-http-agent
RUST_LOG=agentverse_react=debug cargo run -p example-hello-agent
```

### What Gets Logged

**`LlmRunner`** (every LLM call, `debug` level):
```
>>>>>>>>>> LLM PROMPT BEGIN <<<<<<<<<<
...
>>>>>>>>>> LLM RESPONSE BEGIN <<<<<<<<<<
...
```

**`ToolRegistry`** (every tool call, `debug` level):
```
Executing tool  tool="calculator" args=...
Tool result     tool="calculator" result=...
```

**`ReActStrategy`** (each loop iteration, `info` level):
```
Thought only, continuing  iteration=1
Tool executed             iteration=2 tool="calculator"
Strategy completed        iteration=3
```

### Common Debugging Scenarios

**`Config(Missing("provider.api_key is required"))` at startup:**
- This error is only raised for `ProviderConfig::OpenAI` when `base_url` is `None` (i.e., real OpenAI endpoint). Set `MODEL_API_KEY` or provide `base_url`.
- For local endpoints, `api_key` can be empty as long as `base_url` is set.

**Agent not responding:**
1. Check `MODEL_BASE_URL` and `MODEL_API_KEY`.
2. Verify the model server is running: `curl $MODEL_BASE_URL/models`.
3. Enable debug logging: `RUST_LOG=debug` — you'll see the full LLM prompt.

**Tool not being called:**
1. `RUST_LOG=debug` — confirm the tool schema appears in the prompt.
2. Check that the tool name in the prompt matches `tool.name()` exactly.

---

## Quick Reference

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_BASE_URL` | `http://localhost:9090/v1` | OpenAI-compatible LLM backend |
| `MODEL_API_KEY` | *(empty)* | API key; optional for local `base_url` |
| `MODEL_NAME` | model-specific | Model identifier |
| `ANTHROPIC_API_KEY` | — | Anthropic API key |
| `HOST` | `0.0.0.0` | HTTP server bind address |
| `PORT` | `3000` | HTTP server port |
| `API_KEY` | *(unset)* | Bearer token required by all HTTP routes when set |
| `RUST_LOG` | `info` | Log level filter |
| `LOG_FORMAT` | *(text)* | Set to `json` for structured JSON output |

### Key Crates and Types

| Crate | Key Types |
|-------|-----------|
| `agentverse` | `LlmRunner`, `Config`, `ProviderConfig`, `PromptRegistry`, `RunStrategy`, `AsyncTool`, `ModelError` |
| `agentverse-agent` | `Agent`, `AgentError` |
| `agentverse-strategy` | `build()`, `StrategyKind` |
| `agentverse-session` | `SqliteSessionMemory`, `SessionMemory`, `SessionManager`, `SessionId` |
| `agentverse-logging` | `init()` |
| `agentverse-react` | `ReActStrategy` |
| `agentverse-plan` | `PlanStrategy`, `HierarchicalStrategy` |
| `agentverse-router` | `StrategyRouter` |
| `agentverse-guardrails` | `check_prompt`, `check_output`, `RateLimiter` |
| `agentverse-tools` | `ToolRegistry`, `Calculator`, `DateTimeTool`, `FileSearch`, `HttpClient`, `ShellTool`, `WebSearch` |
| `agentverse-integration` | `IntegrationRuntime`, `Event` |

### ProviderConfig Enum

```rust
pub enum ProviderConfig {
    OpenAI { model_name: String, api_key: String, base_url: Option<String> },
    Anthropic { model_name: String, api_key: String },
    Gemini { model_name: String, api_key: String },
}
```

`api_key` is validated as non-empty only for `OpenAI` when `base_url` is `None`.

### ModelError Variants

```rust
pub enum ModelError {
    ApiError(String),
    Timeout(String),
    InvalidResponse(String),
    RateLimited(String),
    CircuitOpen(String),
}
```
