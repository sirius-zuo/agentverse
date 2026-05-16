# Build & E2E Testing Guide

## Prerequisites

| Requirement | Minimum | Tested |
|---|---|---|
| Rust | 1.75 | 1.95.0 |
| Cargo | bundled with Rust | 1.95.0 |
| protobuf-compiler | any | 3.x (for CI; not needed for local build) |

No external services are required to build or run unit tests. A running LLM endpoint is only needed for the e2e examples.

---

## Building

```bash
# Build the entire workspace
cargo build --workspace

# Build only the library crates (no examples)
cargo build -p agentverse -p agentverse-react -p agentverse-tools

# Build a specific example
cargo build -p example-hello-agent
```

All crates are path-local — no registry dependencies beyond `crates.io`. The first build downloads dependencies via Cargo; subsequent builds are incremental.

---

## Unit Tests

```bash
# Run all workspace tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p agentverse
cargo test -p agentverse-react
cargo test -p agentverse-tools
```

Expected output: **141 tests, 0 failures** across all crates.

Tests are pure-unit (no network). Integration tests that require a live endpoint are gated by feature flags or explicit `#[ignore]`.

### Formatting check (required by CI)

```bash
cargo fmt --check        # check only
cargo fmt                # apply in place
```

---

## Workspace Layout

```
AgentVerse/
├── avs-core/           # agentverse — ModelProvider trait, memory, prompt registry
├── avs-guardrails/     # agentverse-guardrails — prompt injection / output filtering
├── avs-react/          # agentverse-react — ReActStrategy + CycleSkeleton
├── avs-plan/           # agentverse-plan — plan-and-execute strategy
├── avs-router/         # agentverse-router — strategy router
├── avs-tools/          # agentverse-tools — Calculator, FileSearch, HttpClient, DateTimeTool
├── avs-memory/         # agentverse-memory — memory traits
├── avs-memory-lancedb/ # LanceDB vector memory backend
├── avs-memory-pgvector/# pgvector memory backend
├── avs-mcp/            # MCP protocol implementation
├── avs-server/         # REST server exposing agents via HTTP
├── avs-integration/    # cross-crate integration tests
└── examples/
    ├── hello-agent/        # simplest: no tools, one question
    ├── rag-qa/             # Calculator tool
    ├── web-search-agent/   # FileSearch tool
    └── code-review-agent/  # FileSearch + Calculator
```

---

## E2E Examples

All four examples use `ReActStrategy` and make real LLM calls. They are configured entirely via environment variables — no code changes needed to switch providers or models.

### Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `MODEL_BASE_URL` | no | `http://localhost:9090/v1` | OpenAI-compatible base URL |
| `MODEL_NAME` | no | `Qwen3.6-35B-A3B-GGUF` | Model name passed in the request |
| `MODEL_API_KEY` | no | _(empty)_ | Bearer token; omit for local endpoints |
| `LLAMA_DISABLE_THINKING` | no | `0` | Set to `1` to send `chat_template_kwargs: {"enable_thinking": false}` — required for Qwen3 on llama.cpp |
| `PROJECT_DIR` | no | `/Users/jinzuo/projects/AgentVerse` | Root path used by FileSearch examples |

### Local llama.cpp Setup

The examples were validated against llama.cpp serving Qwen3. Start llama.cpp with:

```bash
llama-server \
  --model /path/to/Qwen3.6-35B-A3B-GGUF \
  --port 9090 \
  --ctx-size 8192
```

Verify it is healthy:

```bash
curl http://localhost:9090/v1/models
# → {"models": [{"name": "unsloth/Qwen3.6-35B-A3B-GGUF", ...}]}
```

> **Why `LLAMA_DISABLE_THINKING=1`?** Qwen3 models have thinking mode enabled by default in llama.cpp. When the ReAct loop appends the model's previous response as an assistant message and sends it back, llama.cpp rejects the request with HTTP 400 ("Assistant response prefill is incompatible with enable_thinking"). Setting this env var adds `chat_template_kwargs: {"enable_thinking": false}` to every request, disabling the internal thinking mode while preserving normal response quality.

---

### Example 1 — hello-agent

No tools. Simplest smoke test for the provider connection.

**Run:**
```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=unsloth/Qwen3.6-35B-A3B-GGUF \
LLAMA_DISABLE_THINKING=1 \
cargo run -p example-hello-agent
```

**Request sent to the model** (first and only iteration):
```json
{
  "model": "unsloth/Qwen3.6-35B-A3B-GGUF",
  "messages": [
    {
      "role": "system",
      "content": "You are a helpful AI assistant.\nYou are concise and accurate. Never claim to have done something you haven't.\nIf you don't know something, say so.\n\nAlways end your response with:\nAnswer: <your answer>"
    },
    {
      "role": "user",
      "content": "Hello! Introduce yourself briefly and name two things you can help with."
    }
  ],
  "chat_template_kwargs": {"enable_thinking": false}
}
```

**Expected output:**
```
Hello Agent — model: unsloth/Qwen3.6-35B-A3B-GGUF @ http://localhost:9090/v1
> Hello! Introduce yourself briefly and name two things you can help with.

Agent: I am Qwen, an AI developed by Alibaba Group's Tongyi Lab, and I can help with
       complex problem solving and creative/professional writing.

[tokens] input=81 output=124 cache_read=0 cache_write=0
```

---

### Example 2 — rag-qa (Calculator)

Uses the `calculator` tool. Exercises the full ReAct tool-call loop.

**Run:**
```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=unsloth/Qwen3.6-35B-A3B-GGUF \
LLAMA_DISABLE_THINKING=1 \
cargo run -p example-rag-qa
```

**System prompt tool section** (rendered from DEFAULT_SYSTEM_TEMPLATE):
```
Available tools:

- calculator: Perform arithmetic calculations: add, subtract, multiply, divide
  Parameters:
    - operation (required): Arithmetic operation
    - a (required): First operand
    - b (required): Second operand

Always respond in this exact format:
Thought: <your reasoning>
Action: <tool_name>
Action Input: <json args>

When you have the final answer:
Thought: <your reasoning>
Answer: <final answer>
```

**ReAct loop trace** (two tool calls, one answer):

| Iteration | Model output | Framework action |
|---|---|---|
| 1 | `Thought: I need to multiply 42 by 37 first.\nAction: calculator\nAction Input: {"operation":"multiply","a":42,"b":37}` | Calls `Calculator.execute({"operation":"multiply","a":42,"b":37})` → `{"result":1554}` |
| 2 | `Thought: Now add 15.\nAction: calculator\nAction Input: {"operation":"add","a":1554,"b":15}` | Calls `Calculator.execute({"operation":"add","a":1554,"b":15})` → `{"result":1569}` |
| 3 | `Thought: I have the final result.\nAnswer: 1569` | Returns `CycleResult { answer: "1569", ... }` |

**Expected output:**
```
RAG QA Agent — model: unsloth/Qwen3.6-35B-A3B-GGUF @ http://localhost:9090/v1
Tool: Calculator
> What is 42 multiplied by 37, then add 15 to the result?

Agent: 1569

[tokens] input=949 output=152 cache_read=0 cache_write=0
```

---

### Example 3 — web-search-agent (FileSearch)

Uses the `file_search` tool to locate `.rs` files on disk.

**Run:**
```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=unsloth/Qwen3.6-35B-A3B-GGUF \
LLAMA_DISABLE_THINKING=1 \
PROJECT_DIR=/Users/jinzuo/projects/AgentVerse \
cargo run -p example-web-search-agent
```

**Tool definition in system prompt:**
```
- file_search: Search for files matching a pattern in a directory
  Parameters:
    - path (required): Directory to search in
    - pattern (required): Glob pattern (e.g., '*.txt', '**/*.rs')
```

**ReAct loop trace:**

| Iteration | Model output | Framework action |
|---|---|---|
| 1 | `Action: file_search\nAction Input: {"path":"/Users/.../avs-core/src","pattern":"*.rs"}` | Returns `{"matches":[...9 paths...],"count":9}` |
| 2 | `Answer: The .rs files are: agent.rs, builder.rs, ...` | Loop ends |

**Expected output:**
```
Web Search Agent — model: unsloth/Qwen3.6-35B-A3B-GGUF @ http://localhost:9090/v1
Tool: FileSearch (project: /Users/jinzuo/projects/AgentVerse)
> Use the file_search tool to find all .rs files in /Users/jinzuo/projects/AgentVerse/avs-core/src and list their names.

Agent: The .rs files in /Users/jinzuo/projects/AgentVerse/avs-core/src are:
1. agent.rs
2. builder.rs
3. config.rs
4. error.rs
5. example.rs
6. lib.rs
7. model.rs
8. prompt.rs
9. tool.rs

[tokens] input=544 output=151 cache_read=0 cache_write=0
```

---

### Example 4 — code-review-agent (FileSearch + Calculator)

Uses both tools in a single loop: search, count, arithmetic.

**Run:**
```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=unsloth/Qwen3.6-35B-A3B-GGUF \
LLAMA_DISABLE_THINKING=1 \
PROJECT_DIR=/Users/jinzuo/projects/AgentVerse \
cargo run -p example-code-review-agent
```

**ReAct loop trace:**

| Iteration | Tool | Input | Result |
|---|---|---|---|
| 1 | `file_search` | `{"path":".../avs-react/src","pattern":"*.rs"}` | 4 files found |
| 2 | `calculator` | `{"operation":"multiply","a":4,"b":100}` | `{"result":400}` |
| 3 | — | — | `Answer: 400` |

**Expected output:**
```
Code Review Agent — model: unsloth/Qwen3.6-35B-A3B-GGUF @ http://localhost:9090/v1
Tools: FileSearch + Calculator
> Find all .rs files in /Users/jinzuo/projects/AgentVerse/avs-react/src using file_search,
  count how many there are, then use calculator to multiply that count by 100.

Agent: 400

[tokens] input=933 output=161 cache_read=0 cache_write=0
```

---

## Using a Different Provider

The `OpenAICompatible` provider works with any OpenAI-compatible endpoint. To use a hosted API:

```bash
# OpenAI
MODEL_BASE_URL=https://api.openai.com/v1 \
MODEL_NAME=gpt-4o-mini \
MODEL_API_KEY=sk-... \
cargo run -p example-hello-agent

# Any other compatible endpoint
MODEL_BASE_URL=https://your-endpoint/v1 \
MODEL_NAME=your-model \
MODEL_API_KEY=your-key \
cargo run -p example-rag-qa
```

Do not set `LLAMA_DISABLE_THINKING=1` for non-llama.cpp endpoints — the `chat_template_kwargs` field is only meaningful to llama.cpp and will be ignored or rejected elsewhere.

For the Anthropic provider, use `AnthropicProvider` instead of `OpenAICompatible` in the example source — it handles prompt caching automatically via the `anthropic-beta: prompt-caching-2024-07-31` header.

---

## Token Usage

Every example prints a `[tokens]` line after the agent completes:

```
[tokens] input=949 output=152 cache_read=0 cache_write=0
```

| Field | Source | Meaning |
|---|---|---|
| `input` | `usage.prompt_tokens` | Total prompt tokens across all iterations |
| `output` | `usage.completion_tokens` | Total generated tokens across all iterations |
| `cache_read` | `usage.prompt_tokens_details.cached_tokens` | Tokens served from the provider cache (OpenAI KV cache / Anthropic prompt cache) |
| `cache_write` | Anthropic only | Tokens written to prompt cache in this request |

For multi-iteration runs (tool-using examples), these are **cumulative** across the entire ReAct loop — each `generate()` call's `UsageStats` is accumulated via `AddAssign` in `CycleSkeleton`.
