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

The fastest way to try AgentVerse is to run one of the bundled examples. They use the OpenAI API out of the box.

```bash
OPENAI_API_KEY=sk-xxx cargo run -p example-hello-agent
```

> **Using a local LLM with CLI agents?** The example agents are hardcoded for OpenAI. To use a local LLM, either edit the example code (two lines) or use the [HTTP Server](#run-the-http-server-optional) below, which supports any OpenAI-compatible endpoint without code changes.

### Run the HTTP Server (Optional)

The HTTP server exposes an agent as an API — useful for web frontends, mobile apps, or shared infrastructure.

```bash
cargo build -p agentverse-server

# Using OpenAI API
MODEL_BASE_URL=https://api.openai.com MODEL_API_KEY=sk-xxx \
  cargo run -p agentverse-server

# Using a local LLM (llama.cpp on port 9090, server on 8080)
MODEL_BASE_URL=http://127.0.0.1:9090 MODEL_API_KEY="" \
  cargo run -p agentverse-server
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
| `MODEL_API_KEY` | *(empty)* | API key (required for OpenAI, optional for local LLM) |
| `MODEL_NAME` | *(inferred)* | Model identifier (e.g. `gpt-4`, `phi3-mini`) |
| `API_KEY` | *(empty)* | Server auth token (Bearer token for `/invoke` on port 8080) |
| `CONFIG_PATH` | *(none)* | Path to YAML config file |
| `RUST_LOG` | `info` | Logging level |

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
| `agentverse-tools` | Built-in tools (Calculator, DateTime, FileSearch, HttpClient) |
| `agentverse-mcp` | MCP client for external tool servers |
| `agentverse-guardrails` | Security layer (prompt/output/rate limiting) |
| `agentverse-integration` | Slack, Webhook adapters |
| `agentverse-server` | Standalone HTTP server |

## Examples

| Example | Strategy | Description |
|---|---|---|
| `hello-agent` | ReAct | Simple agent, no tools |
| `slack-hr-assistant` | ReAct + Adapter | Slack integration with built-in tools |
| `rag-qa` | ReAct + Vector DB | Document Q&A via HttpClient |
| `web-search-agent` | Plan-and-Execute | Web research with HttpClient + FileSearch |
| `code-review-agent` | Hierarchical | Code analysis with FileSearch + Calculator |

```bash
# Run an example (uses OpenAI API by default)
OPENAI_API_KEY=sk-xxx cargo run -p example-hello-agent
```

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
