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
- [Using the HTTP Server](#using-the-http-server)
- [Aether Adapter — Stdio Mode](#aether-adapter--stdio-mode)
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
    ├── code-review-agent/ # Hierarchical planning
    └── anthropic-react/   # Anthropic Claude with prompt caching
```

### Key Concepts

| Concept | Crate | Description |
|---------|-------|-------------|
| **Agent** | `agentverse` | Main entry point — holds config, memory, prompt registry |
| **Config** | `agentverse` | Model settings, prompts directory, system prompt |
| **PromptRegistry** | `agentverse` | Template engine (Minijinja) + example storage |
| **Tool** | `agentverse` | `AsyncTool` trait (primary); `SyncTool` for CPU-bound tools wrapped via `SyncToolAdapter` |
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

# Start the OpenAI-compatible server
./build/bin/llama-server -m models/phi3/Phi-3-mini-4k-instruct-q4_k_M.gguf \
  --host 127.0.0.1 \
  --port 9090
```

Then set your environment variables:

```bash
export MODEL_BASE_URL=http://127.0.0.1:9090
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
use agentverse::{Agent, Config, ProviderConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    // Optional: load prompts from a directory
    let prompts_dir = PathBuf::from("prompts");

    let config = Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
            base_url: Some("http://127.0.0.1:9090/v1".to_string()),
        },
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

---

## Multi-LLM Provider Configuration

AgentVerse supports multiple LLM providers through a unified interface. The `ProviderConfig` enum allows you to switch providers without changing agent code.

### ProviderConfig Enum

| Variant | Fields | Use Case |
|---------|--------|----------|
| `OpenAI` | `model_name`, `api_key`, `base_url` | OpenAI-compatible endpoints (llama.cpp, Ollama, vLLM, etc.) |
| `Anthropic` | `model_name`, `api_key` | Claude models via Anthropic API |
| `Gemini` | `model_name`, `api_key` | Google Gemini models via Gemini API |

### Configuration Examples

**OpenAI** (or any OpenAI-compatible endpoint):
```yaml
provider:
  type: openai
  model_name: "gpt-4"
  api_key: "sk-xxx"
  base_url: "http://127.0.0.1:9090/v1"  # llama.cpp, Ollama, etc.
```

**Anthropic**:
```yaml
provider:
  type: anthropic
  model_name: "claude-3-sonnet-20240229"
  api_key: "sk-ant-xxx"
```

**Gemini**:
```yaml
provider:
  type: gemini
  model_name: "gemini-pro"
  api_key: "your-gemini-api-key"
```

### ProviderWrapper: Retry and Circuit Breaker

The `ProviderWrapper` adds resilience to all providers with configurable retry and circuit breaker logic.

**Default Settings:**
- **Retries**: 3 attempts with exponential backoff (500ms base delay)
- **Circuit Breaker**: Opens after 5 consecutive failures, resets after 30 seconds
- **Retryable Errors**: `RateLimited` (HTTP 429) and `ApiError` (HTTP 5xx)
- **Non-Retryable Errors**: `InvalidResponse`, `CircuitOpen`, `Timeout`

**Custom Configuration:**
```rust
let provider = OpenAICompatible::from_config(config)?;
let wrapper = ProviderWrapper::new(provider)
    .with_retries(5, 1000)  // 5 retries, 1s base delay
    .with_circuit_breaker(10, 60);  // Open after 10 failures, reset after 60s
```

**Error Variants:**
- `ModelError::RateLimited` — HTTP 429 response
- `ModelError::CircuitOpen` — Circuit breaker is open, retry later
- `ModelError::ApiError` — Other API errors (non-429)
- `ModelError::InvalidResponse` — Response parsing failed
- `ModelError::Timeout` — Request timed out

### Using Providers in Code

```rust
use agentverse::{Agent, Config, ProviderConfig};

// OpenAI
let config = Config {
    provider: ProviderConfig::OpenAI {
        model_name: "gpt-4".to_string(),
        api_key: std::env::var("OPENAI_API_KEY").unwrap(),
        base_url: Some("http://127.0.0.1:9090/v1".to_string()),
    },
    max_messages: 50,
    tools: vec![],
    prompts_dir: None,
    system_prompt: None,
};

let agent = Agent::from_config(config)?;

// Anthropic
let config = Config {
    provider: ProviderConfig::Anthropic {
        model_name: "claude-3-sonnet-20240229".to_string(),
        api_key: std::env::var("ANTHROPIC_API_KEY").unwrap(),
    },
    max_messages: 50,
    tools: vec![],
    prompts_dir: None,
    system_prompt: None,
};

// Gemini
let config = Config {
    provider: ProviderConfig::Gemini {
        model_name: "gemini-pro".to_string(),
        api_key: std::env::var("GEMINI_API_KEY").unwrap(),
    },
    max_messages: 50,
    tools: vec![],
    prompts_dir: None,
    system_prompt: None,
};
```

---

## Writing Tools

Tools extend agent capabilities. The primary interface is `AsyncTool` — implement it directly for any tool that does I/O, and use `SyncToolAdapter` to wrap simple CPU-bound tools.

### Async Tool (primary interface)

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
        // Call external API, etc.
        Ok(json!({ "weather": format!("Sunny in {city}") }))
    }
}
```

### Sync Tool (CPU-bound only)

Use `SyncTool` only for pure computation — never for I/O. Wrap with `SyncToolAdapter` before registering.

```rust
use agentverse::{SyncTool, ToolResult};
use serde_json::{json, Value};

pub struct MyCalculator;

impl SyncTool for MyCalculator {
    fn name(&self) -> &str { "my_calculator" }
    fn description(&self) -> &str { "Adds two numbers" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "number" }
            },
            "required": ["a", "b"]
        })
    }
    fn execute(&self, args: Value) -> ToolResult {
        let a = args["a"].as_f64().unwrap_or(0.0);
        let b = args["b"].as_f64().unwrap_or(0.0);
        Ok(json!({ "result": a + b }))
    }
}
```

### ToolRegistry

`ToolRegistry` is the unified async tool store. All tools register as `AsyncTool`; sync tools are wrapped via `SyncToolAdapter`.

```rust
use agentverse_tools::{ToolRegistry, SyncToolAdapter, Calculator, DateTimeTool, ShellTool};
use std::time::Duration;

let mut registry = ToolRegistry::new();

// Sync tools — wrap with SyncToolAdapter
registry.register_with_category(SyncToolAdapter(Calculator), "math");
registry.register_with_category(SyncToolAdapter(DateTimeTool), "utility");

// Native async tools — register directly
registry.register_with_category(WeatherTool, "network");

// Shell tool — sandboxed subprocess execution
registry.register_with_category(
    ShellTool::new(
        "./workspace",                    // working directory
        Duration::from_secs(30),         // per-command timeout
        vec!["sudo".into(), "rm".into()], // blocked commands
    ),
    "shell",
);

// Filter to a subset for a specific agent
let math_tools = registry.filter_category("math");

// Execute a tool by name
let result = registry.execute("calculator", json!({"operation": "add", "a": 1, "b": 2})).await;

// Get tool schemas for prompt injection
let schemas: Vec<serde_json::Value> = registry.schema();
```

### ShellTool

`ShellTool` runs arbitrary shell commands via `tokio::process::Command`. It is designed for dev-tool and local-automation agents where the model is trusted.

```rust
use agentverse_tools::ShellTool;
use std::time::Duration;

let tool = ShellTool::new(
    "./project",                          // commands start here (not a filesystem sandbox)
    Duration::from_secs(30),             // process killed after 30s
    vec!["sudo".into(), "curl".into()],  // reject these binaries
);
```

The agent calls it with a `command` string:
```json
{ "command": "cargo test -p my-crate" }
```

The response always includes `stdout`, `stderr`, and `exit_code`. Non-zero exit codes are returned as `Ok` — the agent decides how to handle them.

**Security note:** `workdir` sets the initial current directory but does not restrict filesystem access. Pair with a blocked-command list and OS-level isolation for stronger sandboxing.

### Wiring tools into ReActStrategy

```rust
use agentverse_react::ReActStrategy;
use agentverse_tools::{ToolRegistry, SyncToolAdapter, Calculator};
use std::sync::Arc;
use tokio::sync::Mutex;

let mut tools = ToolRegistry::new();
tools.register_with_category(SyncToolAdapter(Calculator), "math");

let strategy = ReActStrategy::new(
    prompt_registry,
    model,
    tools,
    Arc::new(Mutex::new(memory)),
    10, // max iterations
);
```

---

## Prompt Engineering

AgentVerse uses a **three-layer prompt system** designed to maximise LLM prompt cache reuse across multi-turn agent loops.

### Template Roles

| Layer | File | Contains | Cache behaviour |
|---|---|---|---|
| System | `system.j2` | Agent identity + rules | Cached in the system block — paid once per session |
| Preamble | `react.j2` | Tool descriptions + format instructions + few-shot examples | Inserted as `messages[0]` once; sits inside the stable prefix captured by the penultimate-message cache breakpoint |
| Conversation | *(memory)* | Thought / Action / Tool Result / Answer exchanges | Volatile; only the current message is uncharged |

**Why this split matters:** Repeating tools and examples in every user message defeats prefix caching. By placing them in a stable first message that never changes, they are paid for on the first request and cached on every subsequent turn.

### Directory Layout

**ReAct strategy:**
```
prompts/
  system.j2              # Identity + rules only — no tool descriptions here
  react.j2               # Tools + format + {% if examples %}...{% endif %}
  react_examples.toml    # Few-shot examples for react.j2 (set name: "react_examples")
  examples.toml          # General examples available to other strategies
```

**Hierarchical strategy:**
```
prompts/
  system.j2                   # Identity + rules
  hierarchical.j2             # Decomposition prompt (registers as "strategies.hierarchical.decompose")
  hierarchical_examples.toml  # Decomposition few-shot (set name: "hierarchical_examples")
  examples.toml
```

**Plan-and-Execute strategy:**
```
prompts/
  system.j2             # Identity + rules
  plan_and_execute.j2   # Planning prompt (registers as "strategies.plan_and_execute")
  plan_examples.toml    # Planning few-shot (set name: "plan_examples")
  examples.toml
```

> **File stem → registry key mapping:** `hierarchical.j2` and `plan_and_execute.j2` are automatically mapped to their canonical strategy names (`"strategies.hierarchical.decompose"` and `"strategies.plan_and_execute"`) when loaded from a `prompts/` directory.

### Template Files (.j2)

Minijinja templates. `system.j2` contains only identity and rules:

```jinja2
{# prompts/system.j2 #}
You are a precise calculator assistant that solves problems step-by-step.
Always verify every arithmetic operation with the calculator tool.
Never guess a result — compute it.
```

`react.j2` contains tool descriptions, format instructions, and optional examples. It is rendered **once** at agent startup and prepended to the conversation as a stable user message:

```jinja2
{# prompts/react.j2 #}
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

All example files use the `[[example]]` array-of-tables format. The file stem becomes the example-set name:

```toml
# prompts/react_examples.toml  →  example set "react_examples"
[[example]]
input = "What is 6 * 7?"
output = "Thought: I need to multiply.\nAction: calculator\nAction Input: {\"operation\": \"multiply\", \"a\": 6, \"b\": 7}"

[[example]]
input = "What is 100 / 4?"
output = "Thought: I need to divide.\nAction: calculator\nAction Input: {\"operation\": \"divide\", \"a\": 100, \"b\": 4}"
```

For decomposition examples (hierarchical), use `output` to hold the expected JSON array of sub-goals:

```toml
# prompts/hierarchical_examples.toml  →  example set "hierarchical_examples"
[[example]]
input = "Audit avs-core/src for security issues"
output = "[\"Find all .rs files in avs-core/src\", \"Search for unsafe blocks\", \"Check error handling patterns\"]"
```

### Wiring the Registry

```rust
use agentverse::{PromptConfig, PromptRegistry};
use std::sync::Arc;

let registry = Arc::new(
    PromptRegistry::from_config(&PromptConfig {
        // CARGO_MANIFEST_DIR resolves at compile time to the crate root,
        // so the path works regardless of where `cargo run` is invoked from.
        prompts_dir: Some(
            concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string(),
        ),
        ..Default::default()
    })
    .expect("prompt config"),
);
```

Do **not** use `PromptRegistry::default()` in examples — it ignores all files in `prompts/`.

### Prompt Registry API

```rust
// Render a template with context variables
let mut context = std::collections::HashMap::new();
context.insert("tools".to_string(), serde_json::json!("calculator"));
let rendered = registry.render("system", context)?;

// Access a named example set
let examples = registry.get_examples("react_examples");

// Check whether a react.j2 file was loaded (used internally by ReActStrategy)
let has_preamble = registry.has_react_template();

// Add templates or examples programmatically
registry.add_template("custom", "You are {{ persona }}.");
registry.add_examples("my_set", vec![example]);
```

### Default Templates

These are always available and can be overridden by placing the corresponding file in `prompts/`:

| Registry name | Override file | Used by |
|---|---|---|
| `system` | `system.j2` | Every strategy |
| `react` | `react.j2` | `ReActStrategy` preamble |
| `strategies.plan_and_execute` | `plan_and_execute.j2` | `PlanStrategy` |
| `strategies.hierarchical.decompose` | `hierarchical.j2` | `HierarchicalStrategy` |
| `router` | `router.j2` | `RouterStrategy` |

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
        provider: agentverse::ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: "test-key".to_string(),
            base_url: None,
        },
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
EXPOSE 8080
CMD ["./agentverse-server"]
```

Build and run:

```bash
docker build -t agentverse-server .
docker run -p 8080:8080 \
  -e MODEL_BASE_URL=https://api.openai.com \
  -e MODEL_API_KEY=sk-xxx \
  -e API_KEY=my-secret-token \
  agentverse-server
```

### Production Configuration

```yaml
# config.yaml
host: "0.0.0.0"
port: 8080
agent:
  provider:
    type: openai
    model_name: "gpt-4"
    api_key: "sk-xxx"
    base_url: "https://api.openai.com/v1"
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
curl -X POST http://localhost:8080/invoke \
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

## Aether Adapter — Stdio Mode

AgentVerse agents can be driven by [Aether](https://github.com/sirius-zuo/aether), an independent multi-agent orchestration framework. No Aether code is imported into AgentVerse — the integration is a thin transport layer in `avs-server` that speaks the Envelope wire protocol on stdin/stdout.

### How it works

When `agentverse --stdio` is passed, the binary skips the HTTP server and enters a protocol loop:

```
stdin  →  [read Envelope JSON line]  →  dispatch  →  [write Envelope JSON line]  →  stdout
                                           │
                                     agent.invoke()
```

All `tracing` logs go to **stderr** — stdout is reserved for the protocol.

### Envelope protocol

The wire format is newline-delimited JSON. One JSON object per line, no pretty-printing.

```
{"id":"<uuid>","kind":"invoke","payload":{"message":"..."},"metadata":{"trace_id":"..."}}\n
{"id":"<uuid>","kind":"result","payload":{"message":"..."},"metadata":{"model":"...","provider":"openai","tokens_input":"0","tokens_output":"0"}}\n
```

**Kind values and their meaning:**

| Kind | Direction | Description |
|------|-----------|-------------|
| `invoke` | Aether → AgentVerse | Run the agent with `payload.message` |
| `result` | AgentVerse → Aether | Successful response in `payload.message` |
| `error` | AgentVerse → Aether | Failure — `payload.error` contains the message |
| `ping` | Aether → AgentVerse | Health check |
| `pong` | AgentVerse → Aether | Health check response (same `id`, null payload) |

**Response metadata fields** (always present in `result` / `error` responses):

| Key | Description |
|-----|-------------|
| `model` | Model name from `MODEL_NAME` env var |
| `provider` | Inferred from base URL: `openai` / `anthropic` / `gemini` |
| `tokens_input` | Input token count (currently `"0"` — wired when `invoke()` returns usage) |
| `tokens_output` | Output token count (currently `"0"`) |

Aether sets `trace_id`, `workflow_id`, and `node` in outgoing `invoke` envelopes. AgentVerse copies these through to the `result` response so Aether's correlation table can match them.

### Running manually (for debugging)

You can exercise the adapter by hand without Aether:

```bash
cargo build -p agentverse-server

# Start the adapter
MODEL_API_KEY=sk-xxx MODEL_BASE_URL=http://localhost:9090/v1 MODEL_NAME=my-model \
  ./target/debug/agentverse --stdio

# In another terminal — paste JSON lines and press Enter:
echo '{"id":"00000000-0000-0000-0000-000000000001","kind":"ping","payload":null,"metadata":{}}' | \
  MODEL_API_KEY=sk-xxx ./target/debug/agentverse --stdio
# → {"id":"00000000-...","kind":"pong","payload":null,"metadata":{}}
```

### Passing configuration to Aether

When Aether spawns an AgentVerse node via `StdioFactory`, it passes environment variables through the `envs` map:

```rust
StdioFactory {
    node_name: "my-agent".to_string(),
    command: "/path/to/agentverse".to_string(),
    args: vec!["--stdio".to_string()],
    envs: HashMap::from([
        ("MODEL_API_KEY".to_string(), "sk-xxx".to_string()),
        ("MODEL_BASE_URL".to_string(), "http://localhost:9090/v1".to_string()),
        ("MODEL_NAME".to_string(), "my-model".to_string()),
    ]),
}
```

To use a YAML config file instead of environment variables, pass `CONFIG_PATH`:

```rust
envs: HashMap::from([
    ("CONFIG_PATH".to_string(), "/path/to/agent-config.yaml".to_string()),
]),
```

### Relevant files

| File | Description |
|------|-------------|
| `avs-server/src/envelope.rs` | `Envelope`, `EnvelopeKind`, `read_envelope`, `write_envelope` |
| `avs-server/src/stdio_adapter.rs` | `run_stdio` loop, `dispatch` (Ping/Invoke), `response_metadata` |
| `avs-server/src/main.rs` | `--stdio` CLI flag (clap), pre-HTTP branch |
| `avs-server/tests/stdio_test.rs` | Integration tests: Ping→Pong, Invoke→result, EOF→clean exit |

### Testing the adapter

```bash
# Unit tests (codec + config)
cargo test -p agentverse-server envelope

# Integration tests (spawns real binary)
cargo build -p agentverse-server
cargo test -p agentverse-server --test stdio_test
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
| `MODEL_BASE_URL` | `http://localhost:9090` | OpenAI-compatible LLM backend endpoint |
| `MODEL_API_KEY` | *(empty)* | API key |
| `MODEL_NAME` | *(inferred)* | Model identifier |
| `API_KEY` | *(empty)* | Server auth token |
| `CONFIG_PATH` | *(none)* | YAML config file path |
| `RUST_LOG` | `info` | Logging level |

### Key Crates

| Crate | Key Types |
|-------|-----------|
| `agentverse` | `Agent`, `Config`, `ProviderConfig`, `AgentBuilder`, `PromptRegistry`, `Example`, `SyncTool`, `AsyncTool`, `ModelError` |
| `agentverse-react` | `ReActStrategy` |
| `agentverse-plan` | `PlanStrategy`, `HierarchicalStrategy` |
| `agentverse-router` | `StrategyRouter`, `StrategyName` |
| `agentverse-guardrails` | `check_prompt`, `check_output`, `RateLimiter` |
| `agentverse-tools` | `ToolRegistry`, `SyncToolAdapter`, `Calculator`, `DateTimeTool`, `FileSearch`, `HttpClient`, `ShellTool` |
| `agentverse-integration` | `SlackAdapter`, `WebhookAdapter` |

### ProviderConfig Enum

```rust
pub enum ProviderConfig {
    OpenAI { model_name: String, api_key: String, base_url: Option<String> },
    Anthropic { model_name: String, api_key: String },
    Gemini { model_name: String, api_key: String },
}
```

### ModelError Variants

```rust
pub enum ModelError {
    ApiError(String),           // General API errors
    Timeout(String),            // Request timeout
    InvalidResponse(String),    // Response parsing failed
    RateLimited(String),        // HTTP 429
    CircuitOpen(String),        // Circuit breaker is open
}
```

---

## Next Steps

1. **Start with examples**: `cargo run -p example-hello-agent`
2. **Read the design spec**: `docs/superpowers/specs/2026-05-09-prompt-management-design.md`
3. **Implement a custom tool**: See `avs-tools/src/` for examples
4. **Deploy the server**: `cargo run -p agentverse-server`
