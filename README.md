# AgentVerse

Lightweight, extensible AI Agent framework in Rust.

AgentVerse provides a modular architecture for building AI agents with support for multiple orchestration strategies (ReAct, Plan-and-Execute, Hierarchical), built-in tools, prompt engineering with Jinja2 templates, security guardrails, and an HTTP server for remote access.

## Quick Start

### Prerequisites

- **Rust 1.75+** — `rustup install stable`
- **LLM backend** — one of:
  - **OpenAI API** — get an API key from [platform.openai.com](https://platform.openai.com)
  - **Local LLM** — [llama.cpp](https://github.com/ggerganov/llama.cpp), [Ollama](https://ollama.ai), or any OpenAI-compatible server

### Option 1: Run a Demo Agent (CLI)

> **Note:** The example agents currently use the OpenAI API directly. For local LLM support with custom URLs, use the HTTP server (Option 2) or the `AgentBuilder` API.

```bash
# Using OpenAI
OPENAI_API_KEY=sk-xxx cargo run -p example-hello-agent
```

### Option 2: Run the HTTP Server (Recommended for Local LLM)

The HTTP server supports any OpenAI-compatible endpoint via environment variables.

**Step 1: Start your LLM backend**

```bash
# llama.cpp
./server -m models/llama.gguf --host 127.0.0.1 --port 9090

# Ollama (runs on port 11434 by default)
ollama serve
```

**Step 2: Build and run the server**

```bash
cargo build -p agentverse-server

# With local LLM (llama.cpp on port 9090)
MODEL_BASE_URL=http://127.0.0.1:9090 MODEL_API_KEY="" \
  cargo run -p agentverse-server

# With local LLM (Ollama on port 11434)
MODEL_BASE_URL=http://127.0.0.1:11434/v1 MODEL_API_KEY="ollama" \
  cargo run -p agentverse-server

# With OpenAI
MODEL_BASE_URL=https://api.openai.com \
  MODEL_API_KEY=sk-xxx \
  cargo run -p agentverse-server
```

> **Other OpenAI-compatible services?** Just set `MODEL_BASE_URL` to your service's base URL:
> - **vLLM**: `http://localhost:8000/v1`
> - **LM Studio**: `http://localhost:1234/v1`
> - **Groq**: `https://api.groq.com/openai/v1`
> - **Together AI**: `https://api.together.xyz/v1`

### Test the Server

```bash
# Health check
curl http://localhost:9090/health

# Invoke the agent
curl -X POST http://localhost:9090/invoke \
  -H "Content-Type: application/json" \
  -d '{"user_id": "user1", "message": "Hello, agent!"}'
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `MODEL_BASE_URL` | `http://localhost:8080` | OpenAI-compatible API endpoint |
| `MODEL_API_KEY` | *(empty)* | API key (required for OpenAI, optional for local) |
| `MODEL_NAME` | *(inferred from URL)* | Model identifier |
| `API_KEY` | *(empty)* | Server auth token (Bearer token for `/invoke`) |
| `CONFIG_PATH` | *(none)* | Path to YAML config file |
| `RUST_LOG` | `info` | Logging level |

## Configuration File

```yaml
# config.yaml
host: "0.0.0.0"
port: 8080
agent:
  model_api_key: ""
  model_name: "gpt-4"
  strategy: ReAct
  max_iterations: 10
guardrails:
  enabled: true
  max_requests_per_minute: 60
```

Run with: `CONFIG_PATH=config.yaml cargo run -p agentverse-server`

## Server API

### `GET /health`

Returns server health and model info.

```json
{"status": "healthy", "model": "http://localhost:8080"}
```

### `GET /ready`

Returns readiness status.

```json
{"status": "ready"}
```

### `POST /invoke`

Invoke the agent with a user message.

```bash
curl -X POST http://localhost:9090/invoke \
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
# Run an example
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
