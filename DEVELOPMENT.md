# AgentVerse Developer Guide

Complete guide for developing, testing, and deploying agents with AgentVerse.

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Development Setup](#development-setup)
- [Creating a Custom Agent](#creating-a-custom-agent)
- [Writing Tools](#writing-tools)
- [Prompt Engineering](#prompt-engineering)
- [Testing Strategies](#testing-strategies)
- [Deploying Agents](#deploying-agents)
- [Adding Long-Term Memory](#adding-long-term-memory)
- [Integrating External Systems](#integrating-external-systems)
- [Using the HTTP Server](#using-the-http-server)
- [Debugging & Observability](#debugging--observability)

---

## Architecture Overview

AgentVerse is a modular Rust framework organized as a Cargo workspace:

```
AgentVerse/
├── avs-core/              # Core: Agent, Config, PromptRegistry, Memory, Tool traits
├── avs-react/             # ReAct strategy loop
├── avs-plan/              # Plan-and-Execute + Hierarchical strategies
├── avs-router/            # Dynamic strategy routing
├── avs-tools/             # Built-in tools (Calculator, DateTime, FileSearch, HttpClient)
├── avs-mcp/               # MCP client for external tool servers
├── avs-guardrails/        # Security: prompt injection, output filtering, rate limiting
├── avs-integration/       # Slack, Webhook adapters
├── avs-memory/            # Memory traits (Memory, ShortTermMemory)
├── avs-memory-lancedb/    # LanceDB-backed long-term memory
├── avs-memory-pgvector/   # pgvector-backed long-term memory
├── avs-server/            # Standalone HTTP server
└── examples/              # Example agents
    ├── hello-agent/       # Simple agent, no tools
    ├── slack-hr-assistant/ # Slack integration
    ├── rag-qa/            # Document Q&A
    ├── web-search-agent/  # Plan-and-Execute
    └── code-review-agent/ # Hierarchical planning
```

### Key Concepts

| Concept | Crate | Description |
|---------|-------|-------------|
| **Agent** | `agentverse` | Main entry point — holds config, memory, prompt registry |
| **Config** | `agentverse` | Model settings, prompts directory, system prompt |
| **PromptRegistry** | `agentverse` | Template engine (Minijinja) + example storage |
| **Tool** | `agentverse` | `SyncTool` / `AsyncTool` traits for agent capabilities |
| **Strategy** | `agentverse-react`, `agentverse-plan` | Orchestration loops (ReAct, Plan-and-Execute, Hierarchical) |
| **Router** | `agentverse-router` | Dynamic strategy selection via LLM |
| **Guardrails** | `agentverse-guardrails` | Prompt injection detection, output filtering, rate limiting |
| **Memory** | `agentverse-memory` | Short-term memory with configurable capacity |

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
cargo clippy --workspace -- -D warnings

# Format all code
cargo fmt --all

# Build the server binary
cargo build -p agentverse-server

# Build all examples
cargo build --examples
```

### Local LLM Development

For local development without API costs, run llama.cpp as an OpenAI-compatible server:

```bash
# Download llama.cpp
git clone https://github.com/ggerganov/llama.cpp.git
cd llama.cpp
cmake -B build
cmake --build build --config Release -n

# Download a model (e.g., Phi-3)
python3 models/download.py --repo-id microsoft/Phi-3-mini-4k-instruct --local-dir models/phi3

# Start the OpenAI-compatible server (use a port different from the server's 9090)
./build/bin/llama-server -m models/phi3/Phi-3-mini-4k-instruct-q4_k_M.gguf \
  --host 127.0.0.1 \
  --port 8080
```

Then set your environment variables:

```bash
export MODEL_BASE_URL=http://127.0.0.1:8080
export MODEL_API_KEY=""
export MODEL_NAME="phi3-mini"
```

---

## Creating a Custom Agent

### Option 1: Quick Start with `Config`

The simplest way to create an agent:

```rust
// Cargo.toml
[dependencies]
agentverse = { path = "path/to/avs-core" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

// src/main.rs
use agentverse::{Agent, Config};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    // Optional: load prompts from a directory
    let prompts_dir = PathBuf::from("prompts");

    let config = Config {
        model_api_key: std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY not set"),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
        prompts_dir: Some(prompts_dir.to_string_lossy().to_string()),
        system_prompt: None,
    };

    let agent = Agent::from_config(config).unwrap();

    // Invoke the agent
    let result = agent.invoke("user1", "Hello, what can you do?").await.unwrap();
    println!("Agent: {}", result);
}
```

### Option 2: Programmatic Builder

For more control over configuration:

```rust
use agentverse::{Agent, AgentBuilder};

let agent = Agent::builder()
    .config(config)
    .system_prompt("You are a specialized assistant for X.")
    .prompt_dir("prompts/")
    .build()?;
```

### Option 3: Prompt-Enabled Agent

To explicitly configure prompts:

```rust
use agentverse::{Agent, Config, PromptConfig};

let config = Config { /* ... */ };
let prompt_config = PromptConfig {
    system_prompt: Some("You are a code reviewer.".to_string()),
    prompts_dir: Some("prompts/".to_string()),
    templates: std::collections::HashMap::new(),
    examples: std::collections::HashMap::new(),
};

let agent = Agent::from_config_with_prompts(config, &prompt_config)?;
```

### Running Your Agent

```bash
MODEL_API_KEY=sk-xxx cargo run --bin my-agent
```

---

## Writing Tools

Tools extend agent capabilities. Implement either `SyncTool` or `AsyncTool`.

### Sync Tool (simple, blocking)

```rust
use agentverse::{SyncTool, ToolResult};
use serde_json::Value;

pub struct MyTool;

impl SyncTool for MyTool {
    fn name(&self) -> &str {
        "my_tool"
    }

    fn description(&self) -> &str {
        "Does something useful"
    }

    fn execute(&self, args: Value) -> Result<ToolResult, String> {
        let query = args["query"].as_str().unwrap_or("");
        let result = format!("Result for: {}", query);
        Ok(ToolResult::success(result))
    }
}
```

### Async Tool (with I/O)

```rust
use agentverse::{AsyncTool, ToolResult};
use serde_json::Value;

pub struct WeatherTool;

#[async_trait::async_trait]
impl AsyncTool for WeatherTool {
    fn name(&self) -> &str {
        "weather"
    }

    fn description(&self) -> &str {
        "Get weather for a city"
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, String> {
        let city = args["city"].as_str().unwrap_or("unknown");
        // Call external API, etc.
        let result = format!("Weather in {}: sunny", city);
        Ok(ToolResult::success(result))
    }
}
```

### Registering Tools

```rust
use agentverse_tools::ToolRegistry;
use my_tool::{MyTool, WeatherTool};

let mut registry = ToolRegistry::new();
registry.register(Box::new(MyTool));
registry.register(Box::new(WeatherTool));

// Later, execute by name:
let result = registry.execute("my_tool", json!({"query": "hello"})).await;
```

---

## Prompt Engineering

AgentVerse uses a **hybrid prompt system**: embedded defaults + optional file overrides.

### Directory Structure

```
my-agent/
├── Cargo.toml
├── src/main.rs
└── prompts/
    ├── system.j2          # System prompt template
    ├── react.j2           # ReAct strategy template
    ├── examples.toml      # Few-shot examples
    └── react_examples.toml # Strategy-specific examples
```

### Template Files (.j2)

Minijinja templates support all Jinja2 features:

```jinja2
{# prompts/react.j2 #}
You are using the ReAct pattern: Think → Act → Observe.

Available tools:
{{ tools }}

{% if examples %}
Here are some examples:
{% for example in examples %}
User: {{ example.input }}
Assistant: {{ example.output }}
{% endfor %}
{% endif %}

Respond in this format:
Thought: [your reasoning]
Action: [tool name]
Action Input: [tool arguments as JSON]
```

### Example Files (.toml)

Two TOML formats are supported:

**Array-of-tables format** (multiple examples):
```toml
[[example]]
input = "What is 2+2?"
output = "Thought: I can calculate this.\nAction: calculator\nAction Input: {\"expression\": \"2 + 2\"}"

[[example]]
input = "What time is it?"
output = "Thought: I can get the current time.\nAction: datetime\nAction Input: {\"format\": \"%Y-%m-%d %H:%M:%S\"}"
```

**Single example format**:
```toml
[input]
input = "What is the capital of France?"
output = "Paris"
```

### Prompt Registry API

```rust
use agentverse::PromptRegistry;

// Create from config (loads defaults + directory)
let registry = PromptRegistry::from_config(&prompt_config)?;

// Add templates programmatically
registry.add_template("custom", "You are {{ persona }}.");

// Add examples
registry.add_examples("my_examples", vec![example]);

// Render a template
let mut context = std::collections::HashMap::new();
context.insert("tools".to_string(), serde_json::json!("calculator, weather"));
context.insert("conversation".to_string(), serde_json::json!("User: hello"));
let rendered = registry.render("react", context)?;
```

### Default Templates

The following templates are always available:

| Template Name | Description |
|---------------|-------------|
| `system` | System prompt |
| `react` / `strategies.react` | ReAct strategy |
| `strategies.plan_and_execute` | Plan-and-Execute strategy |
| `strategies.hierarchical.decompose` | Hierarchical decomposition |
| `router` | Strategy router |

---

## Testing Strategies

### Unit Tests

Write unit tests alongside your code:

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

Create `tests/` directory in your crate:

```rust
// tests/integration_test.rs
use agentverse::{Agent, Config};

#[test]
fn test_agent_creation() {
    let config = Config {
        model_api_key: "test-key".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 10,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };
    assert!(Agent::from_config(config).is_ok());
}
```

### Mock Testing

Use `mockall` for mocking the `ModelProvider` trait:

```rust
use mockall::mock;

mock! {
    pub ModelProvider {}
    #[async_trait::async_trait]
    impl agentverse::ModelProvider for ModelProvider {
        async fn generate(&self, prompt: &str, tools: Option<Vec<serde_json::Value>>) -> Result<String, agentverse::ModelError>;
    }
}
```

### Running Tests

```bash
# All workspace tests
cargo test --workspace

# Single crate
cargo test -p agentverse

# Single test
cargo test -p agentverse test_agent_creation

# With output
cargo test --workspace -- --nocapture
```

---

## Deploying Agents

### Option 1: Standalone Binary

Compile your agent as a standalone binary:

```bash
# Build release binary
cargo build --release -p example-hello-agent

# Run
./target/release/example-hello-agent
```

### Option 2: HTTP Server

Deploy the `agentverse-server` for remote access:

```bash
# Build
cargo build --release -p agentverse-server

# Run with environment variables
MODEL_BASE_URL=https://api.openai.com \
MODEL_API_KEY=sk-xxx \
MODEL_NAME=gpt-4 \
API_KEY=my-secret-token \
RUST_LOG=info \
./target/release/agentverse-server
```

### Option 3: Docker

Create a `Dockerfile`:

```dockerfile
FROM rust:1.75-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p agentverse-server

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/agentverse-server .
EXPOSE 9090
CMD ["./agentverse-server"]
```

Build and run:

```bash
docker build -t agentverse-server .
docker run -p 9090:9090 \
  -e MODEL_BASE_URL=https://api.openai.com \
  -e MODEL_API_KEY=sk-xxx \
  -e API_KEY=my-secret-token \
  agentverse-server
```

### Production Configuration

```yaml
# config.yaml
host: "0.0.0.0"
port: 9090
agent:
  model_api_key: "sk-xxx"
  model_name: "gpt-4"
  strategy: ReAct
  max_iterations: 10
guardrails:
  enabled: true
  max_requests_per_minute: 60
```

Run with: `CONFIG_PATH=config.yaml cargo run -p agentverse-server`

---

## Adding Long-Term Memory

### LanceDB Memory

```rust
use agentverse_lancedb::{LanceDbMemory, LanceDbConfig};

let config = LanceDbConfig {
    uri: "/tmp/agentverse-vector-db".to_string(),
    table_name: "memories".to_string(),
};

let memory = LanceDbMemory::new(config).await?;
```

### pgvector Memory

```rust
use agentverse_pgvector::{PgVectorMemory, PgVectorConfig};

let config = PgVectorConfig {
    connection_string: "postgresql://user:pass@localhost/agentverse".to_string(),
    table_name: "memories".to_string(),
};

let memory = PgVectorMemory::new(config).await?;
```

### Memory API

```rust
use agentverse::Memory;

// Store a memory
memory.store(query, embedding).await?;

// Retrieve similar memories
let results = memory.retrieve(query, top_k).await?;
```

---

## Integrating External Systems

### Slack Adapter

```rust
use agentverse_integration::SlackAdapter;

let adapter = SlackAdapter::new(
    agent,
    &std::env::var("SLACK_BOT_TOKEN").expect("SLACK_BOT_TOKEN"),
    &std::env::var("SLACK_SIGNING_SECRET").expect("SLACK_SIGNING_SECRET"),
    3000,
);

adapter.start().await.expect("Failed to start Slack adapter");
```

### Webhook Adapter

```rust
use agentverse_integration::WebhookAdapter;

let adapter = WebhookAdapter::new(
    agent,
    "/webhook",
    Some("my-secret-token"),
);

adapter.start().await.expect("Failed to start Webhook adapter");
```

---

## Using the HTTP Server

### API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Server health and model info |
| `/ready` | GET | Readiness check |
| `/invoke` | POST | Invoke the agent |
| `/swagger-ui` | GET | API documentation |

### Invoke Endpoint

```bash
curl -X POST http://localhost:9090/invoke \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my-secret-key" \
  -d '{"user_id": "user1", "message": "Hello, agent!"}'
```

Response:

```json
{
  "message": "Hello! How can I help you?",
  "user_id": "user1"
}
```

### Error Responses

```json
// Bad request (empty message)
{"error": "message must not be empty"}

// Too many requests (rate limited)
{"error": "Rate limit exceeded: 60 requests per minute"}

// Prompt guardrail triggered (prompt injection)
{"error": "Prompt injection detected"}

// Output guardrail triggered (unsafe output)
{"error": "Output filtered"}

// Internal error
{"error": "Model error: API error"}
```

---

## Debugging & Observability

### Logging

AgentVerse uses `tracing` for structured logging:

```bash
# Verbose output
RUST_LOG=debug cargo run -p agentverse-server

# JSON structured logs
RUST_LOG=json cargo run -p agentverse-server

# Filter by component
RUST_LOG=agentverse=info,agentverse_guardrails=debug cargo run -p agentverse-server
```

### Available Log Levels

| Level | Use Case |
|-------|----------|
| `trace` | Detailed execution flow |
| `debug` | Strategy steps, tool calls |
| `info` | Request/response summary |
| `warn` | Guardrail warnings |
| `error` | Failures, API errors |

### Common Debugging Scenarios

**Agent not responding:**
1. Check `MODEL_BASE_URL` and `MODEL_API_KEY`
2. Verify the model server is running: `curl $MODEL_BASE_URL/v1/models`
3. Enable debug logging: `RUST_LOG=debug`

**Guardrail blocking requests:**
1. Check the guardrail error in logs
2. Verify prompt doesn't contain injection patterns
3. Temporarily disable guardrails for testing (not recommended for production)

**Tool execution failing:**
1. Check tool description in the prompt
2. Verify JSON argument format
3. Use `RUST_LOG=debug` to see tool call details

---

## Quick Reference

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_BASE_URL` | `http://localhost:8080` | OpenAI-compatible API endpoint |
| `MODEL_API_KEY` | *(empty)* | API key |
| `MODEL_NAME` | *(inferred)* | Model identifier |
| `API_KEY` | *(empty)* | Server auth token |
| `CONFIG_PATH` | *(none)* | YAML config file path |
| `RUST_LOG` | `info` | Logging level |

### Key Crates

| Crate | Key Types |
|-------|-----------|
| `agentverse` | `Agent`, `Config`, `AgentBuilder`, `PromptRegistry`, `Example`, `SyncTool`, `AsyncTool` |
| `agentverse-react` | `ReActStrategy` |
| `agentverse-plan` | `PlanStrategy`, `HierarchicalStrategy` |
| `agentverse-router` | `StrategyRouter`, `StrategyName` |
| `agentverse-guardrails` | `check_prompt`, `check_output`, `RateLimiter` |
| `agentverse-tools` | `Calculator`, `DateTimeTool`, `FileSearch`, `HttpClient`, `ToolRegistry` |
| `agentverse-integration` | `SlackAdapter`, `WebhookAdapter` |

---

## Next Steps

1. **Start with examples**: `cargo run -p example-hello-agent`
2. **Read the design spec**: `docs/superpowers/specs/2026-05-09-prompt-management-design.md`
3. **Implement a custom tool**: See `avs-tools/src/` for examples
4. **Deploy the server**: `cargo run -p agentverse-server`
