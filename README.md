# AgentVerse

Lightweight, extensible AI Agent framework in Rust.

AgentVerse provides a modular architecture for building AI agents with support for multiple orchestration strategies (ReAct, Plan-and-Execute, Hierarchical), built-in tools, prompt engineering with Jinja2 templates, security guardrails, and an HTTP server for remote access.

## Quick Start

### Prerequisites

- **Rust 1.75+** — `rustup install stable`
- **LLM API** — any OpenAI-compatible endpoint:
  - **OpenAI API** — get a key from [platform.openai.com](https://platform.openai.com)
  - **Local LLM** — [llama.cpp](https://github.com/ggerganov/llama.cpp), [Ollama](https://ollama.ai), or similar

### Run an Example Agent

The fastest way to try AgentVerse is to run one of the bundled examples. They use the `ProviderConfig::OpenAI` default, which works with any OpenAI-compatible endpoint (OpenAI API, llama.cpp, Ollama, vLLM, etc.).

```bash
OPENAI_API_KEY=sk-xxx cargo run -p example-hello-agent
```

> **Using a local LLM?** Set `MODEL_BASE_URL` to point to your endpoint — the example agents use the same `Config` structure as the HTTP server, so no code changes needed.

### Run the HTTP Server (Optional)

The HTTP server exposes an agent as an API — useful for web frontends, mobile apps, or shared infrastructure.

```bash
cargo build -p agentverse-server

# Using OpenAI API
MODEL_BASE_URL=https://api.openai.com MODEL_API_KEY=sk-xxx \
  cargo run -p agentverse-server

# Using a local LLM (llama.cpp on port 9090, server on 8080)
MODEL_BASE_URL=http://127.0.0.1:9090 \
  cargo run -p agentverse-server
# (MODEL_API_KEY is optional for local LLMs — empty key is accepted)
```

Test it:

```bash
# Health check
curl http://localhost:8080/health

# Invoke the agent
curl -X POST http://localhost:8080/invoke \
  -H "Content-Type: application/json" \
  -d '{"user_id": "user1", "message": "Hello, agent!"}'
```

> **Other OpenAI-compatible services?** Set `MODEL_BASE_URL` to your endpoint:
> - **Ollama**: `http://127.0.0.1:11434/v1`
> - **vLLM**: `http://localhost:8000/v1`
> - **LM Studio**: `http://localhost:1234/v1`
> - **Groq**: `https://api.groq.com/openai/v1`
> - **Together AI**: `https://api.together.xyz/v1`

### Run as an Aether Node (Stdio Adapter)

The `agentverse` binary can be driven by [Aether](https://github.com/sirius-zuo/aether) — an independent multi-agent orchestration framework — over a newline-delimited JSON Envelope protocol on stdin/stdout.

```bash
# Build the binary
cargo build -p agentverse-server

# Run in stdio adapter mode (Aether manages process lifecycle)
AGENTVERSE_BIN=/path/to/agentverse
MODEL_API_KEY=sk-xxx MODEL_BASE_URL=http://localhost:9090/v1 MODEL_NAME=my-model \
  $AGENTVERSE_BIN --stdio
```

In `--stdio` mode the binary:
- Reads `Invoke` / `Ping` Envelopes from stdin (one JSON object per line)
- Responds with `Result` / `Pong` / `Error` Envelopes on stdout
- Exits cleanly on EOF (Aether drops stdin when done)
- Writes all logs to **stderr** so stdout stays clean for the protocol

You never invoke `--stdio` manually — Aether spawns the process automatically via `StdioFactory`. See the [Aether project](https://github.com/sirius-zuo/aether) and its `examples/agentverse-pipeline` for a working end-to-end example.

## Multi-LLM Provider Support

AgentVerse supports multiple LLM providers through a unified interface. The `ProviderConfig` enum allows you to switch providers without changing agent code.

### Supported Providers

| Provider | Use Case | Example Model |
|---|---|---|
| **OpenAI** | API-compatible with OpenAI's chat completions (works with llama.cpp, Ollama, vLLM, etc.) | `gpt-4`, `phi3-mini`, `llama-3` |
| **Anthropic** | Claude models via Anthropic's API | `claude-3-opus`, `claude-3-sonnet` |
| **Gemini** | Google's Gemini models via Gemini API | `gemini-pro`, `gemini-1.5-flash` |

### Configuration File

```yaml
# config.yaml
host: "0.0.0.0"
port: 8080
agent:
  provider:
    type: openai  # or "anthropic" or "gemini"
    model_name: "gpt-4"
    api_key: "sk-xxx"
    base_url: "http://127.0.0.1:9090/v1"  # only for OpenAI
  max_iterations: 10
guardrails:
  enabled: true
  max_requests_per_minute: 60
```

#### Provider Examples

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

> **Note:** The `base_url` field is only required for OpenAI-compatible providers (e.g., llama.cpp, Ollama). Anthropic and Gemini use fixed endpoints.

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `MODEL_BASE_URL` | *(none)* | OpenAI-compatible LLM endpoint (e.g. `https://api.openai.com`, `http://127.0.0.1:9090`) |
| `MODEL_API_KEY` | *(empty)* | API key (required for OpenAI/Anthropic/Gemini, optional for local LLMs) |
| `MODEL_NAME` | *(inferred)* | Model identifier (e.g. `gpt-4`, `phi3-mini`) |
| `API_KEY` | *(empty)* | Server auth token (Bearer token for `/invoke` on port 8080) |
| `CONFIG_PATH` | *(none)* | Path to YAML config file |
| `RUST_LOG` | `info` | Logging level |

When running as an Aether node, pass these same variables in `StdioFactory::envs` — the binary reads them at startup regardless of transport mode.

Run with: `CONFIG_PATH=config.yaml cargo run -p agentverse-server`

## Server API

### `GET /health`

Returns server health and model info.

```json
{"status": "healthy", "model": "http://localhost:9090"}
```

### `GET /ready`

Returns readiness status.

```json
{"status": "ready"}
```

### `POST /invoke`

Invoke the agent with a user message.

```bash
curl -X POST http://localhost:8080/invoke \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my-secret-key" \
  -d '{"user_id": "user1", "message": "Hello, agent!"}'
```

Response:

```json
{"message": "Hello! How can I help you?", "user_id": "user1"}
```

## Crates

| Crate | Description |
|---|---|
| `agentverse` | Core framework — Agent, Config, ToolResult, Memory, PromptRegistry |
| `agentverse-react` | ReAct strategy loop |
| `agentverse-plan` | Plan-and-Execute + Hierarchical strategies |
| `agentverse-router` | Dynamic strategy routing |
| `agentverse-memory` | Layered memory system (short/long term) |
| `agentverse-memory-lancedb` | LanceDB-backed long-term memory |
| `agentverse-memory-pgvector` | pgvector-backed long-term memory |
| `agentverse-tools` | Built-in tools (Calculator, DateTime, FileSearch, HttpClient, ShellTool) + async ToolRegistry |
| `agentverse-mcp` | MCP client for external tool servers |
| `agentverse-guardrails` | Security layer (prompt/output/rate limiting) |
| `agentverse-integration` | Slack, Webhook adapters |
| `agentverse-server` | Standalone HTTP server |

## Examples

| Example | Strategy | Tools | Description |
|---|---|---|---|
| `hello-agent` | ReAct | Calculator, DateTime | Interactive REPL — best starting point |
| `rag-qa` | ReAct | Calculator | Tool-use loop with step-by-step arithmetic |
| `web-search-agent` | ReAct | FileSearch | File search + multi-step reasoning |
| `code-review-agent` | Hierarchical | FileSearch, Calculator | Decompose → plan per sub-goal → execute → synthesize |
| `anthropic-react` | ReAct | Calculator | Anthropic Claude with prompt caching (system + preamble) |
| `slack-hr-assistant` | Plan-and-Execute | — | Slack bot using plan-and-execute via `AgentBuilder` |

```bash
# Local LLM (llama.cpp / Ollama)
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=your-model \
cargo run -p example-hello-agent

# Anthropic Claude
ANTHROPIC_API_KEY=sk-ant-... cargo run -p example-anthropic-react
```

Each example has a `prompts/` directory with `system.j2` (identity), strategy template (e.g. `react.j2`, `hierarchical.j2`), and optional few-shot examples in `.toml` files. See [Prompt Templates](#prompt-templates) below.

## Prompt Templates

AgentVerse uses a three-layer prompt design that maximises LLM prompt cache reuse across multi-turn conversations.

### Template roles

| File | Purpose | Cache behaviour |
|---|---|---|
| `system.j2` | Agent identity + rules | Cached in system block — paid once per session |
| `react.j2` | Tool descriptions + format instructions + few-shot examples | Inserted as the first user message once; sits in the stable prefix captured by the penultimate-message cache breakpoint |
| Conversation messages | Actual Thought / Action / Tool Result / Answer exchanges | Volatile; only the current message is uncharged |

Because `react.j2` never changes within a session, it is effectively free after the first request — the prefix cache serves it on every subsequent turn. `system.j2` is cached independently via the system-block breakpoint.

### Directory layout

Each example ships a `prompts/` directory. The layout depends on the strategy:

**ReAct** (`hello-agent`, `rag-qa`, `web-search-agent`, `anthropic-react`):
```
prompts/
  system.j2              # Identity + rules (no tools here)
  react.j2               # Tools + format + {% if examples %}...{% endif %}
  react_examples.toml    # Few-shot examples injected into react.j2
  examples.toml          # General examples (available to other strategies)
```

**Hierarchical** (`code-review-agent`):
```
prompts/
  system.j2                   # Identity + rules
  hierarchical.j2             # Decomposition prompt → "strategies.hierarchical.decompose"
  hierarchical_examples.toml  # input / output pairs showing decomposition
  examples.toml
```

**Plan-and-Execute** (`slack-hr-assistant`):
```
prompts/
  system.j2             # Identity + rules
  plan_and_execute.j2   # Planning prompt → "strategies.plan_and_execute"
  plan_examples.toml    # input / output pairs showing planning
  examples.toml
```

### Wiring it up

```rust
use agentverse::{PromptConfig, PromptRegistry};

let registry = Arc::new(
    PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(
            concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string(),
        ),
        ..Default::default()
    })
    .expect("prompt config"),
);
```

### Example files

All example files use the `[[example]]` TOML array-of-tables syntax:

```toml
# prompts/react_examples.toml
[[example]]
input = "What is 6 * 7?"
output = "Thought: I need to multiply.\nAction: calculator\nAction Input: {\"operation\": \"multiply\", \"a\": 6, \"b\": 7}"
```

The file stem becomes the example-set name (`react_examples.toml` → `"react_examples"`). The `react.j2` template receives this set automatically via `{{ examples }}`.

## Documentation

- **[Developer Guide](DEVELOPMENT.md)** — Complete guide for developing, testing, and deploying agents with AgentVerse
  - Architecture overview
  - Creating custom agents
  - Writing tools
  - Prompt engineering with templates
  - Testing strategies
  - Deploying to production
  - Adding long-term memory
  - Integrating external systems (Slack, Webhooks)
  - Debugging & observability

## Project Structure

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

## License

MIT
