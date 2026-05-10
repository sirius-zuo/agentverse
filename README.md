# AgentVerse

Lightweight, extensible AI Agent framework in Rust.

## Quick Start

### Local Development (llama.cpp)

```bash
# Start llama.cpp server
./server -m models/llama.gguf --host 127.0.0.1 --port 8080

# Build and run AgentVerse server
cargo build -p agentverse-server
MODEL_BASE_URL=http://localhost:8080 MODEL_API_KEY="" \
  cargo run -p agentverse-server
```

### Production (OpenAI-compatible)

```bash
MODEL_BASE_URL=https://api.openai.com \
MODEL_API_KEY=sk-xxx \
cargo run -p agentverse-server
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
| `agentverse` | Core framework — Agent, Config, ToolResult, Memory |
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
