# AgentVerse

Lightweight, extensible AI Agent framework in Rust.

## Quick Start

```bash
cargo build --release
OPENAI_API_KEY=sk-xxx ./target/release/agentverse
```

## Crates

- `agentverse` — Core framework
- `agentverse-react` — ReAct strategy
- `agentverse-plan` — Plan-and-Execute + Hierarchical strategies
- `agentverse-router` — Dynamic strategy routing
- `agentverse-memory` — Layered memory system
- `agentverse-tools` — Built-in tools
- `agentverse-mcp` — MCP client
- `agentverse-guardrails` — Security layer
- `agentverse-integration` — Slack, Webhook adapters
- `agentverse-server` — Standalone server

## Examples

See `examples/` for: hello-agent, slack-hr-assistant, rag-qa, web-search-agent, code-review-agent
