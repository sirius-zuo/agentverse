# AgentVerse

AgentVerse is a Rust workspace for building async AI agents. It separates the core LLM runner from session management, orchestration strategies, tools, memory backends, and platform integrations. All LLM access goes through a single `Agent` entry point; the HTTP server is an optional sidecar that the agent can spawn itself.

## Architecture

```
your binary
  └── agentverse_agent::Agent
        ├── agentverse_strategy::build() → Arc<dyn RunStrategy>
        │     ├── ReActStrategy  (avs-react)
        │     ├── PlanStrategy   (avs-plan)
        │     └── HierarchicalStrategy (avs-plan)
        ├── LlmRunner  (avs-core)
        ├── ToolRegistry  (avs-tools)
        ├── Memory layers
        │     ├── Layer 1: CacheMemory   — in-process RAM buffer, TTL-evicted
        │     ├── Layer 2: SessionMemory — durable per-user conversation transcript (SQLite/Postgres)
        │     └── Layer 3: LongtermMemory — distilled cross-session knowledge (vector store, optional)
        └── HTTP server (avs-agent `http` feature, optional)
```

`Agent` is the only way to invoke the LLM. You choose a strategy with `agentverse_strategy::build(StrategyKind::*)` and pass it to `Agent::new`. The agent handles session history, memory assembly, prompt construction, and optional HTTP serving.

## What Is Implemented

- **Agent**: `agentverse-agent::Agent` is the single LLM access point. Composes `LlmRunner`, `StrategyKind`, `SessionManager`, and memory layers.
- **Strategies**: ReAct, Plan-and-Execute, Hierarchical planning. Selected at construction via `agentverse-strategy::build`. Strategies are pure `Vec<Message> → String` with no memory coupling.
- **Three-layer memory**: Layer 1 `CacheMemory` (in-process, TTL), Layer 2 `SessionMemory` (durable transcript), Layer 3 `LongtermMemory` (distilled cross-session knowledge, opt-in).
- **Multi-user sessions**: `Agent` routes through `SessionManager` for durable per-user conversation history with ownership enforcement.
- **HTTP sidecar**: `Agent::new(..., enable_http_server: true)` spawns an HTTP server as a background task. The agent can run without it; the server cannot run without the agent.
- **Agent-owned integrations**: `IntegrationRuntime` reads connector config, starts Slack/GitHub/WhatsApp or console connectors, calls an agent handler, and sends responses.
- **Strategies and tools**: `ToolRegistry`, built-in tools, and MCP adapters are wired into strategies at agent construction.

## Quick Start

### Prerequisites

- Rust 1.75+
- An OpenAI-compatible model endpoint, Anthropic API key, or Gemini API key

### Run an Example Agent

```bash
# Local OpenAI-compatible endpoint (no API key required)
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=your-model \
cargo run -p example-hello-agent

# Hosted OpenAI endpoint
MODEL_BASE_URL=https://api.openai.com/v1 \
MODEL_API_KEY=sk-xxx \
MODEL_NAME=gpt-4o \
cargo run -p example-hello-agent
```

Other OpenAI-compatible endpoints work by changing `MODEL_BASE_URL`: Ollama, llama.cpp, vLLM, LM Studio, Groq, Together AI.

### Run an HTTP Agent

`example-http-agent` builds an `Agent` with `enable_http_server=true` and keeps the process alive:

```bash
ANTHROPIC_API_KEY=sk-ant-... \
MODEL_NAME=claude-sonnet-4-6 \
HOST=0.0.0.0 PORT=3000 \
cargo run -p example-http-agent
```

The agent listens on `0.0.0.0:3000` by default.

```bash
curl http://localhost:3000/health
curl http://localhost:3000/ready
```

## HTTP API

### `GET /health`

```json
{"status":"healthy","model":"claude-sonnet-4-6"}
```

### `GET /ready`

```json
{"status":"ready"}
```

### `POST /invoke`

Stateless single-message invocation. Does not persist conversation history.

```bash
curl -X POST http://localhost:3000/invoke \
  -H "Content-Type: application/json" \
  -d '{"user_id":"user1","message":"Hello, agent!"}'
```

Response:

```json
{"message":"Hello! How can I help you?","user_id":"user1"}
```

### Session Routes

Sessions persist per-user conversation history in SQLite by default.

```bash
# Create a session
curl -X POST http://localhost:3000/sessions \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice"}'
# → {"session_id":"<uuid>"}

# Send a message
curl -X POST http://localhost:3000/sessions/<session_id>/messages \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice","message":"Remember that my favorite color is green."}'
# → {"session_id":"<uuid>","reply":"..."}

# Get session metadata
curl 'http://localhost:3000/sessions/<session_id>?user_id=alice'

# End a session
curl -X DELETE http://localhost:3000/sessions/<session_id> \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice"}'
```

Session ownership is enforced: `user_id` is checked against the session before any access.

### `POST /aether/invoke`

Aether-compatible HTTP invoke route.

```bash
curl -X POST http://localhost:3000/aether/invoke \
  -H "Content-Type: application/json" \
  -d '{"id":"<uuid>","kind":"invoke","payload":{"input":"Hello"},"metadata":{}}'
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|---|---:|---|
| `MODEL_BASE_URL` | `http://localhost:9090/v1` | OpenAI-compatible base URL |
| `MODEL_API_KEY` | empty | Provider API key (optional when `MODEL_BASE_URL` is set for local endpoints) |
| `MODEL_NAME` | `gpt-4` | Model name |
| `ANTHROPIC_API_KEY` | — | Anthropic API key (for Anthropic provider) |
| `HOST` | `0.0.0.0` | HTTP server bind address |
| `PORT` | `3000` | HTTP server port |
| `API_KEY` | unset | Optional bearer token required by all HTTP routes when set |
| `RUST_LOG` | `info` | Tracing level filter |
| `LOG_FORMAT` | text | Set to `json` for JSON logs |

### YAML Config

```yaml
agent:
  provider:
    type: openai
    model_name: "gpt-4o"
    api_key: "sk-xxx"
    base_url: "https://api.openai.com/v1"
  max_iterations: 10
```

## Multi-User Sessions

`agentverse-agent` owns the top-level `Agent`. `agentverse-session` provides session data infrastructure only.

```text
Agent::invoke(user_id, session_id, input)
  1. assert_owner(user_id, session_id)
  2. get/rehydrate Layer-1 CacheMemory from Layer-2 SessionMemory if cold
  3. retrieve scored Layer-3 LongtermMemory for the input (optional)
  4. assemble messages: [system + long-term context] + cache + user_input
  5. strategy.run(messages)
  6. append_turn to Layer-1 cache and Layer-2 SessionMemory
  7. async: consolidate turn into Layer-3 LongtermMemory
  8. return assistant text
```

Available session memory backends:

- `SqliteSessionMemory` in `agentverse-session` (default)
- `PostgresSessionMemory` in `agentverse-memory-pgvector`

## Integrations

`agentverse-integration` is owned by the agent. The agent creates an `IntegrationRuntime` and provides a handler; the runtime handles connector I/O.

```rust
use agentverse_integration::{Event, IntegrationRuntime};
use std::sync::Arc;

let agent = Arc::new(Agent::new(/* ... */));
let runtime = IntegrationRuntime::from_config("agent.toml").await?;

runtime
    .run(move |event: Event| {
        let agent = Arc::clone(&agent);
        async move {
            let answer = agent.invoke_stateless(&event.text).await
                .map_err(|e| agentverse::AgentError::Memory(e.to_string()))?;
            Ok::<Event, agentverse::AgentError>(Event { text: answer, ..event })
        }
    })
    .await?;
```

If `agent.toml` is missing, `IntegrationRuntime::from_config` falls back to `ConsoleConnector`.

Supported connectors: Console, Slack, GitHub, WhatsApp.

## Strategies and Tools

Strategies are selected via `agentverse-strategy::build`:

```rust
use agentverse_strategy::{build, StrategyKind};

let strategy = build(StrategyKind::React, runner, prompts, tools, 10);
// StrategyKind::Plan
// StrategyKind::Hierarchical
```

Underlying strategy crates:

- `agentverse-react`: ReAct loop
- `agentverse-plan`: Plan-and-Execute and hierarchical planning
- `agentverse-router`: dynamic strategy routing

Built-in tools in `agentverse-tools`:

- Calculator, DateTime, FileSearch, HttpClient, ShellTool, WebSearch
- MCP tool adapter via `agentverse-mcp`

## Prompt Templates

AgentVerse uses `PromptRegistry` to load embedded defaults and optional `.j2` templates from a prompts directory.

Common layout:

```text
prompts/
  system.j2
  react.j2
  react_examples.toml
```

Example:

```rust
use agentverse::{PromptConfig, PromptRegistry};

let registry = Arc::new(PromptRegistry::from_config(&PromptConfig {
    prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
    ..Default::default()
})?);
```

## Crates

| Crate | Package | Purpose |
|---|---|---|
| `avs-core` | `agentverse` | Core config, errors, prompts, model providers, `LlmRunner`, memory and tool traits |
| `avs-agent` | `agentverse-agent` | Single LLM access point: `Agent` composes `LlmRunner`, strategy, and `SessionManager`; optional HTTP sidecar |
| `avs-strategy` | `agentverse-strategy` | Strategy factory (`build`, `StrategyKind`) and umbrella re-exports |
| `avs-session` | `agentverse-session` | Session model, `SessionManager`, `SessionMemory` trait, `SqliteSessionMemory` |
| `avs-integration` | `agentverse-integration` | Agent-owned connector runtime for console, Slack, GitHub, WhatsApp |
| `avs-react` | `agentverse-react` | ReAct strategy loop |
| `avs-plan` | `agentverse-plan` | Plan-and-Execute and hierarchical strategies |
| `avs-router` | `agentverse-router` | Strategy router |
| `avs-tools` | `agentverse-tools` | Built-in tools and async `ToolRegistry` |
| `avs-mcp` | `agentverse-mcp` | MCP client and tool adapter |
| `avs-memory` | `agentverse-memory` | Layer-1 working buffer (`SimpleMemory`, `AgentMemory`) and `LongTermBackend` trait |
| `avs-memory-lancedb` | `agentverse-memory-lancedb` | LanceDB long-term memory backend |
| `avs-memory-pgvector` | `agentverse-memory-pgvector` | pgvector memory backend and Postgres session store |
| `avs-guardrails` | `agentverse-guardrails` | Prompt, output, action, and rate-limit guardrails |
| `avs-logging` | `agentverse-logging` | Tracing subscriber initialization |

## Examples

| Package | Description |
|---|---|
| `example-hello-agent` | Interactive ReAct REPL with Calculator and DateTime |
| `example-react-calculator` | ReAct calculator demonstration |
| `example-web-search-agent` | Plan-and-Execute web search summary |
| `example-anthropic-react` | Anthropic ReAct agent with prompt-cache usage |
| `example-code-review-agent` | Hierarchical code-review agent with file and shell tools |
| `example-slack-hr-assistant` | IntegrationRuntime-backed Slack/console assistant |
| `example-http-agent` | Agent with `enable_http_server=true`; demonstrates the HTTP sidecar |

Run examples with `cargo run -p <package>`.

## Development

```bash
cargo fmt --all --check
cargo clippy --all -- -D warnings
cargo test --workspace
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for deeper implementation notes.

## Project Structure

```text
AgentVerse/
|-- avs-core/
|-- avs-agent/
|-- avs-session/
|-- avs-strategy/
|-- avs-integration/
|-- avs-react/
|-- avs-plan/
|-- avs-router/
|-- avs-tools/
|-- avs-mcp/
|-- avs-memory/
|-- avs-memory-lancedb/
|-- avs-memory-pgvector/
|-- avs-guardrails/
|-- avs-logging/
`-- examples/
```

## License

MIT
