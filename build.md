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

Expected output: all tests passing, 0 failures. The exact count grows as the codebase does (531 passing as of Wave 3) — don't hardcode a number when checking this locally; a shrinking count across a change is the signal to look for, not a fixed target.

Tests are pure-unit (no network) with a few exceptions, all fully offline via mocking or env-gating rather than live network calls: `avs-eval`'s judge-based regression tests and `avs-memory`'s embedder tests mock HTTP via `httpmock` (see [DEVELOPMENT.md's Eval Harness section](DEVELOPMENT.md#eval-harness)); `avs-memory-pgvector`'s Postgres tests (the `SessionMemory` conformance suite plus the `PgVectorStore` store/search test) require `TEST_DATABASE_URL` to be set against a real instance or they silently skip (see [DEVELOPMENT.md's SessionMemory Conformance Suite section](DEVELOPMENT.md#sessionmemory-conformance-suite)); and `avs-memory-lancedb`'s tests run unconditionally against temporary on-disk LanceDB databases (no network, temp-dir writes only). None makes a live LLM call in CI or in a default local run.

### Formatting and lint checks (required by CI)

```bash
cargo fmt --all --check                              # check only
cargo fmt --all                                       # apply in place
cargo clippy --workspace --all-targets -- -D warnings # matches CI; catches tests/examples too
```

CI also runs two structural fitness checks (`./scripts/check-file-sizes.sh`, `./scripts/check-layering.sh`) and a `cargo-deny` licenses/advisories job — see [DEVELOPMENT.md's CI Fitness Checks section](DEVELOPMENT.md#ci-fitness-checks) for details; none of these are exercised by the commands above.

---

## Workspace Layout

See [DEVELOPMENT.md](DEVELOPMENT.md#architecture-overview) for the authoritative, maintained crate list and descriptions — this section previously drifted out of sync with the real workspace (it referenced an `avs-server` crate and a `rag-qa` example that no longer exist) and is kept intentionally short here to avoid a second copy going stale again.

Crates relevant to this guide's build/test commands: `avs-core`, `avs-agent`, `avs-memory`, `avs-session`, `avs-tools`, `avs-react`, `avs-plan`, `avs-strategy`, `avs-logging`. Full workspace: 20 library crates + `examples/*` (see `Cargo.toml`'s `[workspace] members`).

---

## E2E Examples

The four examples below make real LLM calls and are configured entirely via environment variables — no code changes needed to switch providers or models. Only two (`hello-agent`, `react-calculator`) use `ReActStrategy`; `web-search-agent` uses `PlanStrategy` and `code-review-agent` uses `HierarchicalStrategy` (see each example's own section below).

### Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `MODEL_BASE_URL` | no | `http://localhost:9090/v1` | OpenAI-compatible base URL |
| `MODEL_NAME` | no | `Qwen3.6-35B-A3B-GGUF` | Model name passed in the request |
| `MODEL_API_KEY` | no | _(empty)_ | Bearer token; omit for local endpoints |
| `LLAMA_DISABLE_THINKING` | no | _(unset = disabled)_ | Thinking is disabled by default (`chat_template_kwargs: {"enable_thinking": false}` sent on every request); set to `0` or `false` to leave a model's native thinking mode on |
| `PROJECT_DIR` | no | `/Users/jinzuo/projects/AgentVerse` | Root path used by `code-review-agent`'s `FileSearch`/`ShellTool` |

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

> **`LLAMA_DISABLE_THINKING` is now on by default.** `chat_template_kwargs: {"enable_thinking": false}` is sent on every OpenAI-compatible request unless you explicitly set `LLAMA_DISABLE_THINKING=0` (or `false`) — the examples below no longer need to pass `LLAMA_DISABLE_THINKING=1`. This exists because Qwen3 models have thinking mode enabled by default in llama.cpp, and when the ReAct loop appends the model's previous response as an assistant message and sends it back, llama.cpp rejects the request with HTTP 400 ("Assistant response prefill is incompatible with enable_thinking") unless thinking is disabled. Set `LLAMA_DISABLE_THINKING=0` only if you're deliberately using a model that needs its native thinking mode left on.

---

> **A note on this section's traces:** the four examples below are now interactive REPLs (or take CLI args) rather than fixed single-shot Q&A binaries, so there is no longer one canonical "expected output" transcript to capture ahead of time — actual model output depends on which model you run. What's documented below (run command, tools, strategy, skill-system behavior) is verified directly against each example's current source; the printed transcripts are illustrative of the shape of a session, not a byte-for-byte captured trace.

### Example 1 — hello-agent

`SkillMode::Open` — the `SkillRouter` auto-selects between `math-helper` (Calculator) and `datetime-helper` (DateTimeTool) based on your message, or answers directly with no tool if neither matches. `ReActStrategy`, interactive REPL.

**Run:**
```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=unsloth/Qwen3.6-35B-A3B-GGUF \
cargo run -p example-hello-agent
```

```
Skills loaded: math-helper, datetime-helper, travel-advisor
Type your question and press Enter. Type "exit" or press Ctrl+C to quit.

You: What is 6 * 7?

Agent: 42
```

---

### Example 2 — react-calculator

Replaces the older `rag-qa` example. `Calculator` only, no skill system, `ReActStrategy`, interactive REPL — exercises the full multi-step ReAct tool-call loop directly.

**Run:**
```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=unsloth/Qwen3.6-35B-A3B-GGUF \
cargo run -p example-react-calculator
```

```
Type an arithmetic question. Type "exit" or press Ctrl+C to quit.

You: What is 42 multiplied by 37, then add 15 to the result?

Agent: 1569
```

Internally this drives at least two sequential `Action: calculator` tool calls (multiply, then add) before the model emits `Answer:`.

---

### Example 3 — web-search-agent

Takes CLI args, not an interactive REPL: `cargo run -p example-web-search-agent -- "<topic>" <n>`. `SkillMode::Constrained(["web-search"])`, `WebSearch` tool, `PlanStrategy`. Also demonstrates the Shadow pattern — `skills/user/web-search/` overrides `skills/system/web-search/` (same `name:`) with stricter citation rules, no code change.

**Run:**
```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=unsloth/Qwen3.6-35B-A3B-GGUF \
cargo run -p example-web-search-agent -- "rust async programming" 3
```

```
> Search for 'rust async programming' and summarize the top 3 results.

Agent: <summary of 3 web-search results, with footnote citations per the shadowed skill's rules>
```

---

### Example 4 — code-review-agent

Uses `FileSearch` + `ShellTool` (not Calculator — that changed since this guide was last accurate). Explicit skill binding (`create_session_with_skill("user", "code-review")` — the `SkillRouter` never runs), `HierarchicalStrategy`, interactive REPL. `ShellTool` is sandboxed to `PROJECT_DIR` with a blocked-command list (`rm`, `rmdir`, `mv`, `dd`, `sudo`, `chmod`, `chown`) but — per the tool's own security note — `workdir` is not a real filesystem sandbox; absolute paths and `cd` can still reach outside it.

**Run:**
```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=unsloth/Qwen3.6-35B-A3B-GGUF \
PROJECT_DIR=/path/to/project \
cargo run -p example-code-review-agent
```

```
Code Review Agent — explicit skill binding (code-review)
Active tools: file_search, shell
Type a review request and press Enter. Type "exit" to quit.

Review> Find all .rs files under src/ and summarize what each one does.

Agent:
<file_search + shell-driven review, format per the code-review SKILL.md>
```

`PROJECT_DIR` defaults to `/Users/jinzuo/projects/AgentVerse` (a hardcoded fallback in the example's own source, not something this doc controls) — always set it explicitly to the project you actually want reviewed.

---

## Using a Different Provider

The `openai` provider (`ProviderConfig::openai(model_name, api_key, base_url)`) works with any OpenAI-compatible endpoint — llama.cpp, OpenAI itself, or any other compatible backend — by changing the same three env vars each example already reads:

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
cargo run -p example-react-calculator
```

`LLAMA_DISABLE_THINKING` only affects the `openai` provider's request body (`chat_template_kwargs`) — it is meaningless for non-llama.cpp endpoints, but sending it is harmless (compatible servers ignore unrecognized fields; it is only relevant when targeting llama.cpp specifically). See [DEVELOPMENT.md's Multi-LLM Provider Configuration](DEVELOPMENT.md#multi-llm-provider-configuration) for the full `ProviderConfig` API, including `::anthropic(...)`, `::gemini(...)`, and how to register a provider beyond the three built-ins.

For Anthropic, none of the four examples above use it directly — see `examples/anthropic-react` (`ProviderConfig::anthropic(model_name, api_key)`), which handles prompt caching automatically via the `anthropic-beta` header.

---

## Token Usage

`UsageStats { input_tokens, output_tokens, cache_write_tokens, cache_read_tokens }` (`avs-core/src/model.rs`) is tracked on every provider call and accumulated across a multi-step strategy loop via `AddAssign`, but **none of the four examples above print it** — `Agent::invoke()` returns `AgentOutput` (`Done(String)` or `Interrupted{..}`), which doesn't carry usage. It's still reachable if you need it:

- `LlmRunner::invoke`/`invoke_structured` return `GenerateResponse { content, usage }` directly.
- A strategy's internal `CycleResult { answer, total_usage, .. }` carries the cumulative usage across all ReAct/Plan/Hierarchical iterations in one invocation.
- `SubAgentResult { answer, usage, steps }` (`agentverse-subagent`) surfaces it per-subagent for the multi-agent examples (`project-feasibility`, `business-report`).

If you need per-turn token visibility from a console binary like the ones above, call `LlmRunner` directly instead of going through `Agent::invoke`, or install an OTel meter provider and read `gen_ai.client.token.usage` — see [README.md's Metrics section](README.md#metrics).
