# AgentVerse

AgentVerse is a Rust workspace for building async AI agents. It separates the core LLM runner from session management, orchestration strategies, tools, memory backends, platform integrations, and the HTTP server so agents can run as examples, embedded library code, connector-driven bots, or managed HTTP services.

Current server mode is HTTP-first. Agents can run standalone, expose HTTP APIs, and optionally self-register with an Aether HTTP registry. The old stdio and Unix-socket adapter stories are deprecated and are not part of the current server path.

## What Is Implemented

- **Stateless LLM runner**: `agentverse::LlmRunner` renders prompts and calls model providers through `ConnectionManager`.
- **Multi-user sessions**: `agentverse-agent::Agent` composes `LlmRunner` with `SessionManager` for durable session storage.
- **HTTP server**: `agentverse-server` exposes health, readiness, stateless invoke, Aether invoke, and session routes.
- **Aether HTTP registry client**: `avs-server/src/aether_client.rs` registers and deregisters an HTTP agent when `AETHER_REGISTRY_URL` is set.
- **Agent-owned integrations**: `IntegrationRuntime` reads connector config, starts Slack/GitHub/WhatsApp or console connectors, receives events, calls a handler, and sends responses.
- **Strategies and tools**: ReAct, Plan-and-Execute, hierarchical planning, async `ToolRegistry`, built-in tools, and MCP adapters are available to examples and library users.

Important current limitation: the HTTP server builds a `ToolRegistry`, but `/invoke` and `/aether/invoke` currently call `LlmRunner` directly. Tool-using strategies are available in the strategy crates and examples, but are not yet the server invocation path.

## Quick Start

### Prerequisites

- Rust 1.75+
- An OpenAI-compatible model endpoint, Anthropic API key, or Gemini API key
- `protobuf-compiler` for CI-equivalent builds on some platforms

### Run an Example Agent

Examples use the strategy crates directly and are the fastest way to try tool-using agents.

```bash
# Local OpenAI-compatible endpoint
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=your-model \
cargo run -p example-hello-agent

# OpenAI-compatible hosted endpoint
MODEL_BASE_URL=https://api.openai.com/v1 \
MODEL_API_KEY=sk-xxx \
MODEL_NAME=gpt-4o \
cargo run -p example-hello-agent
```

Other OpenAI-compatible endpoints usually work by changing `MODEL_BASE_URL`, for example Ollama, llama.cpp, vLLM, LM Studio, Groq, or Together AI.

### Run the HTTP Server

The `agentverse-server` package builds a binary named `agentverse`.

```bash
cargo build -p agentverse-server
```

Run against a local OpenAI-compatible endpoint:

```bash
MODEL_BASE_URL=http://127.0.0.1:9090/v1 \
MODEL_API_KEY=local-dummy-key \
MODEL_NAME=your-model \
cargo run -p agentverse-server
```

Run against OpenAI:

```bash
MODEL_BASE_URL=https://api.openai.com/v1 \
MODEL_API_KEY=sk-xxx \
MODEL_NAME=gpt-4o \
cargo run -p agentverse-server
```

The server listens on `127.0.0.1:8080` by default.

```bash
curl http://localhost:8080/health
curl http://localhost:8080/ready
```

`MODEL_API_KEY` must be non-empty for the server because `LlmRunner::from_config` validates provider config. For local endpoints that ignore auth, use a harmless dummy value.

## Server API

### `GET /health`

Returns process health and configured model name.

```json
{"status":"healthy","model":"gpt-4o"}
```

### `GET /ready`

Returns readiness.

```json
{"status":"ready"}
```

### `POST /invoke`

Stateless single-message invocation. This route does not persist conversation history.

```bash
curl -X POST http://localhost:8080/invoke \
  -H "Content-Type: application/json" \
  -d '{"user_id":"user1","message":"Hello, agent!"}'
```

Response:

```json
{"message":"Hello! How can I help you?","user_id":"user1"}
```

### Session Routes

Sessions persist per-user conversation history in the configured `SessionStore`. The server currently uses SQLite by default.

Create a session:

```bash
curl -X POST http://localhost:8080/sessions \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice"}'
```

Response:

```json
{"session_id":"00000000-0000-0000-0000-000000000000"}
```

Send a message in a session:

```bash
curl -X POST http://localhost:8080/sessions/<session_id>/messages \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice","message":"Remember that my favorite color is green."}'
```

Response:

```json
{"session_id":"00000000-0000-0000-0000-000000000000","reply":"..."}
```

Get session metadata:

```bash
curl 'http://localhost:8080/sessions/<session_id>?user_id=alice'
```

End a session:

```bash
curl -X DELETE http://localhost:8080/sessions/<session_id> \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice"}'
```

Session ownership is checked by `Agent::assert_owner(user_id, session_id)` before any session access. Authentication, if enabled, is still a server-wide bearer token and does not yet bind request identity to `user_id`.

### `POST /aether/invoke`

Aether-compatible HTTP invoke route using the local `Envelope` JSON shape.

```bash
curl -X POST http://localhost:8080/aether/invoke \
  -H "Content-Type: application/json" \
  -d '{
    "id":"00000000-0000-0000-0000-000000000000",
    "kind":"invoke",
    "payload":{"input":"Hello from Aether"},
    "metadata":{}
  }'
```

Response:

```json
{
  "id":"00000000-0000-0000-0000-000000000000",
  "kind":"result",
  "payload":{"output":"..."},
  "metadata":{}
}
```

## Configuration

You can configure the server with environment variables or `CONFIG_PATH`.

### Environment Variables

| Variable | Default | Description |
|---|---:|---|
| `MODEL_BASE_URL` | `http://localhost:9090/v1` | OpenAI-compatible base URL for server default config |
| `MODEL_API_KEY` | empty | Provider API key; must be non-empty for the server |
| `MODEL_NAME` | `gpt-4` | Model name for server default config |
| `API_KEY` | unset | Optional bearer token required by all server routes when set |
| `CONFIG_PATH` | unset | YAML config file path |
| `SESSION_STORE` | `sqlite` | Session store backend: `sqlite` or `postgres` |
| `SESSION_DB_URL` | `sqlite:sessions.db` | Database URL for the chosen session store |
| `AETHER_REGISTRY_URL` | unset | Optional Aether HTTP registry base URL |
| `AGENT_NAME` | `agentverse-agent` | Logical name used during Aether registration |
| `RUST_LOG` | `info` | Tracing level filter |
| `LOG_FORMAT` | text | Set to `json` for JSON logs |

### YAML Config

```yaml
host: "127.0.0.1"
port: 8080
agent_name: "agentverse-agent"
aether_registry_url: null
agent:
  provider:
    type: openai
    model_name: "gpt-4o"
    api_key: "sk-xxx"
    base_url: "https://api.openai.com/v1"
  max_iterations: 10
guardrails:
  enabled: true
  max_requests_per_minute: 60
session:
  store: "sqlite"
  database_url: "sqlite:sessions.db"
```

For Postgres session storage:

```yaml
session:
  store: "postgres"
  database_url: "postgres://user:password@localhost:5432/agentverse"
```

Run with:

```bash
CONFIG_PATH=config.yaml cargo run -p agentverse-server
```

## Aether HTTP Registry

AgentVerse no longer uses stdio or Unix sockets for the current Aether management path. When `AETHER_REGISTRY_URL` is set, the server:

1. Starts its normal HTTP API.
2. Posts `name`, `http_url`, and `capabilities` to `{AETHER_REGISTRY_URL}/registry/agents`.
3. Stores the returned `instance_id` in memory.
4. Deregisters best-effort on SIGTERM with `DELETE /registry/instances/{instance_id}`.

If the registry is unreachable, the server logs a warning and continues as a standalone HTTP agent.

```bash
AETHER_REGISTRY_URL=http://localhost:7000 \
AGENT_NAME=research-agent \
MODEL_BASE_URL=http://127.0.0.1:9090/v1 \
MODEL_API_KEY=local-dummy-key \
MODEL_NAME=your-model \
cargo run -p agentverse-server
```

Aether is a lifecycle coordinator, not the business-data path. It discovers and health-checks agents over HTTP; agent payloads are handled by the agent's own HTTP or integration surfaces.

## Multi-User Sessions

`agentverse-agent` owns the top-level `Agent`. `agentverse-session` does not contain an agent; it provides session data infrastructure only.

```text
Entry point
  -> agentverse_agent::Agent
    -> agentverse_session::SessionManager
      -> Arc<dyn SessionStore>
    -> LlmRunner
      -> ConnectionManager
      -> ModelProvider
```

The session agent flow is:

1. `assert_owner(user_id, session_id)` — verify the user owns the session.
2. Load existing messages for `session_id`.
3. Add the new user message in memory.
4. Call `LlmRunner::invoke(messages)`.
5. Persist both messages atomically via `append_turn` after a successful LLM response.
6. Return the assistant text.

Available stores:

- `SqliteSessionStore` in `agentverse-session`, used by the server by default.
- `PostgresSessionStore` in `agentverse-memory-pgvector`, for production use.

Ownership checks are performed by `Agent` through `SessionManager::assert_owner(user_id, session_id)` before any store access. The store itself operates by `session_id` only.

## Integrations

`agentverse-integration` is owned by the agent. It does not drive the agent from outside. The agent creates an `IntegrationRuntime`, provides a handler, and the runtime handles connector I/O.

```rust
use agentverse_integration::{Event, IntegrationRuntime};

let runtime = IntegrationRuntime::from_config("agent.toml").await?;

runtime
    .run(|event: Event| async move {
        let answer = handle_event_text(event.text.clone()).await?;
        Ok(Event { text: answer, ..event })
    })
    .await?;
```

If `agent.toml` is missing, `IntegrationRuntime::from_config` falls back to `ConsoleConnector`.

Example `agent.toml`:

```toml
[integration]
input = "slack"
outputs = ["slack"]

[connector.slack]
port = 3000
bot_token_env = "SLACK_BOT_TOKEN"
signing_secret_env = "SLACK_SIGNING_SECRET"

[connector.github]
port = 3001
token_env = "GITHUB_TOKEN"
webhook_secret_env = "GITHUB_WEBHOOK_SECRET"
```

Supported connector implementations:

- Console
- Slack
- GitHub
- WhatsApp

Connector secrets are environment variable names in config, not secret values.

## Strategies and Tools

Strategies:

- `agentverse-react`: ReAct loop
- `agentverse-plan`: Plan-and-Execute and hierarchical planning
- `agentverse-router`: dynamic strategy routing

Tools:

- Calculator
- DateTime
- FileSearch
- HttpClient
- ShellTool
- WebSearch
- MCP tool adapter via `agentverse-mcp`

`ToolRegistry` stores async tools and supports category tags. Example agents wire tools directly into strategies. The HTTP server's direct invocation routes do not yet execute strategy loops or tool calls.

## Prompt Templates

AgentVerse uses `PromptRegistry` to load embedded defaults and optional `.j2` templates from a prompts directory.

Common example layout:

```text
prompts/
  system.j2
  react.j2
  react_examples.toml
  examples.toml
```

Strategy template names are normalized by file name:

- `react.j2` -> `react` / `strategies.react`
- `plan_and_execute.j2` -> `strategies.plan_and_execute`
- `hierarchical.j2` -> `strategies.hierarchical.decompose`

Example:

```rust
use agentverse::{PromptConfig, PromptRegistry};

let registry = PromptRegistry::from_config(&PromptConfig {
    prompts_dir: Some("examples/hello-agent/prompts".to_string()),
    ..Default::default()
})?;
```

Example `.toml` files use array-of-table entries:

```toml
[[example]]
input = "What is 6 * 7?"
output = "Thought: I should multiply.\nAction: calculator\nAction Input: {\"operation\":\"multiply\",\"a\":6,\"b\":7}"
```

## Crates

| Crate | Package | Purpose |
|---|---|---|
| `avs-core` | `agentverse` | Core config, errors, prompts, model providers, `LlmRunner`, memory and tool traits |
| `avs-agent` | `agentverse-agent` | Top-level multi-user agent orchestrator that composes `LlmRunner` and `SessionManager` |
| `avs-session` | `agentverse-session` | Session model, session manager, store trait, and SQLite session store |
| `avs-server` | `agentverse-server` | HTTP server, auth middleware, session routes, Aether registry client |
| `avs-integration` | `agentverse-integration` | Agent-owned connector runtime for console, Slack, GitHub, WhatsApp |
| `avs-react` | `agentverse-react` | ReAct strategy loop |
| `avs-plan` | `agentverse-plan` | Plan-and-Execute and hierarchical strategies |
| `avs-router` | `agentverse-router` | Strategy router |
| `avs-tools` | `agentverse-tools` | Built-in tools and async `ToolRegistry` |
| `avs-mcp` | `agentverse-mcp` | MCP client and tool adapter |
| `avs-memory` | `agentverse-memory` | Memory helpers and long-term backend traits |
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
| `example-anthropic-react` | Anthropic ReAct example with prompt-cache usage stats |
| `example-code-review-agent` | Hierarchical code-review agent with file and shell tools |
| `example-slack-hr-assistant` | IntegrationRuntime-backed Slack/console assistant |

Run examples with `cargo run -p <package>`.

## Development

```bash
cargo fmt --all --check
cargo clippy --all -- -D warnings
cargo test --all
cargo check --examples
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for deeper implementation notes.

## Project Structure

```text
AgentVerse/
|-- avs-core/
|-- avs-session/
|-- avs-server/
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
|-- examples/
`-- docs/superpowers/specs/
```

## License

MIT
