# Example READMEs Design

**Date:** 2026-06-11
**Status:** Approved

## Overview

Each of the 7 target examples (`hello-agent`, `code-review-agent`, `web-search-agent`, `doc-pipeline`, `support-router`, `business-report`, `project-feasibility`) currently has no README. A reader who clones the repo and opens an example directory has no entry point: no explanation of what the example demonstrates, how to run it, or why it was built.

This spec defines a README for each example. No spec links are included — design background is written inline.

---

## Template

Every README uses these four sections in this order. Depth scales with complexity.

1. **What this shows** — Named design concepts (bold), each with 1–2 sentences. Readable as a standalone index.
2. **How to run** — Environment variable table (name | required | description) + exact `cargo run` command.
3. **How it's built** — Annotated walkthrough of key files and the critical code path. Simple examples: prose paragraphs. Complex examples: subsections + ASCII diagrams + code callouts.
4. **Design background** — Why this example exists, what alternatives were considered, what a production system would add differently.

---

## Per-Example Spec

### `hello-agent`

**Depth:** Short. Two design concepts, four paragraphs of build walkthrough, two paragraphs of background.

**What this shows:**

- **SkillMode::Open + automatic routing** — Skills are discovered from `skills/system/` (built-in: math-helper, datetime-helper) and `skills/user/` (operator-added: travel-advisor). On the first `invoke`, `SkillRouter` scores all candidates against the user message and binds the best match for the session's lifetime. If no skill scores high enough, the agent responds using all skill summaries as soft context without binding.
- **Extend pattern** — A new skill added to `skills/user/` becomes available with no code change. This mirrors how an operator extends a shipped agent at deployment time.

**How to run:**

| Variable | Required | Description |
|---|---|---|
| `MODEL_BASE_URL` | No | OpenAI-compatible API base URL (default: `http://localhost:9090/v1`) |
| `MODEL_NAME` | No | Model ID (default: `Qwen3.6-35B-A3B-GGUF`) |
| `MODEL_API_KEY` | No | API key (default: empty) |

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
cargo run -p example-hello-agent
```

**How it's built:**

- `ToolRegistry` registers `Calculator` (category: `math`) and `DateTimeTool` (category: `utility`).
- `PromptRegistry::from_config` loads `prompts/` — a thin `system.j2` (cross-skill identity) and `react.j2` (ReAct format).
- `SkillConfig::load(skills_dir, SkillMode::Open)` walks `skills/system/` then `skills/user/`, building a skill catalogue.
- `Agent::new(...)` with a ReAct strategy (max 10 steps). `create_session("user")` creates the session — routing hasn't run yet.
- The REPL loop calls `agent.invoke("user", session_id, &input)`. On the first call the router runs, scores each skill's description against the input, and binds the winner. Subsequent calls in the same session use the bound skill.

**Design background:**

This is the baseline example — the simplest complete agent. It was built first to show the routing-to-binding lifecycle in its most transparent form. Using `SkillMode::Open` makes the routing visible: ask a math question and math-helper binds; ask about travel and travel-advisor binds. The Extend pattern (user/ tier) was included to show that the skill directory is layered: `system/` ships with the agent binary, `user/` is added at deployment time without recompilation. In production you would add authentication, persistent session storage (replace `sqlite::memory:` with a real database path), and error recovery for the REPL loop.

---

### `code-review-agent`

**Depth:** Short. Two design concepts, three paragraphs of build walkthrough, two paragraphs of background.

**What this shows:**

- **Explicit skill binding** — `create_session_with_skill("user", "code-review")` binds the skill before the first message. `SkillRouter` never runs; the session is locked to `code-review` from creation.
- **Tool restriction via SKILL.md** — The `code-review` skill declares `agentverse.tools: [file_search, shell]`. Only those two tools appear in the preamble; all other registered tools are invisible to the agent during this session.

**How to run:**

| Variable | Required | Description |
|---|---|---|
| `MODEL_BASE_URL` | No | OpenAI-compatible API base URL (default: `http://localhost:9090/v1`) |
| `MODEL_NAME` | No | Model ID (default: `Qwen3.6-35B-A3B-GGUF`) |
| `MODEL_API_KEY` | No | API key (default: empty) |
| `PROJECT_DIR` | Yes | Directory `ShellTool` uses as working directory |

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
PROJECT_DIR=/path/to/your/project \
cargo run -p example-code-review-agent
```

**How it's built:**

- `FileSearch` and `ShellTool` registered in `ToolRegistry`. `ShellTool` is scoped to `PROJECT_DIR` with a 30-second timeout. Destructive commands (`rm`, `mv`, `dd`, `sudo`, `chmod`, `chown`) are blocked at the tool level. Note: `workdir` is not a filesystem sandbox — absolute paths and symlinks can still escape. For production, run inside a container.
- `SkillConfig::load(skills_dir, SkillMode::Open)` — the skill catalogue is loaded open, but explicit binding (not the router) governs which skill is active.
- Hierarchical strategy (max 10 steps): the agent decomposes the review request into sub-goals (security, performance, style, logic) and executes each as a plan step.
- `create_session_with_skill("user", "code-review")` sets the skill before any message is sent. `SkillMode::Open` + explicit binding is a common combination: the catalogue is open for future extension, but this particular session type doesn't need routing.

**Design background:**

Built to show the "you already know which skill you need" case. When a user launches a code-review agent, there is no ambiguity about intent — routing adds latency for no benefit. Explicit binding also makes the session deterministic: the same skill is always active, regardless of how the user phrases their first message. The tool restriction demonstrates the minimal-privilege benefit of SKILL.md: the agent can read files and run shell commands, but it cannot use a calculator or web search — the tools that would be irrelevant (and potentially distracting) for code review.

---

### `web-search-agent`

**Depth:** Short. Two design concepts, three paragraphs of build walkthrough, two paragraphs of background.

**What this shows:**

- **SkillMode::Constrained** — `SkillMode::Constrained(vec!["web-search"])` makes only skills named `web-search` eligible. Any other skills in `skills/` are invisible to the router, regardless of how many are loaded.
- **Shadow pattern** — `skills/user/web-search/` declares the same `name: web-search` as `skills/system/web-search/`. The user variant (v1.1.0) loads after the system variant and silently replaces it. The result: stricter citation rules activate at the user tier with no code change.

**How to run:**

| Variable | Required | Description |
|---|---|---|
| `MODEL_BASE_URL` | No | OpenAI-compatible API base URL (default: `http://localhost:9090/v1`) |
| `MODEL_NAME` | No | Model ID (default: `Qwen3.6-35B-A3B-GGUF`) |
| `MODEL_API_KEY` | No | API key (default: empty) |

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
cargo run -p example-web-search-agent -- "rust async programming" 3
```

Arguments: `<topic>` (quoted string), `<n>` (number of results to fetch, 1–10).

**How it's built:**

- `WebSearch` tool registered. Plan strategy (max 5 steps): the agent plans which pages to fetch, executes the fetches, then synthesises.
- `SkillConfig::load` walks `skills/system/` first, then `skills/user/`. When a name collision is found (`web-search` appears in both), the user/ entry replaces the system/ entry in the catalogue. `SkillMode::Constrained(vec!["web-search"])` then ensures only this skill is eligible.
- `create_session("user")` + router runs with one eligible skill — binding is deterministic. The agent receives the user/ variant of the skill, which requires numbered footnote citations.

**Design background:**

The Shadow pattern was designed for deployment-time customisation without forking. The base agent ships with a permissive `web-search` skill (system/). A specific deployment that needs compliance-grade citations drops a stricter skill file into `user/` — no recompilation, no code change. `SkillMode::Constrained` complements Shadow: it guarantees that even if an operator adds unrelated skills to `user/`, those skills cannot accidentally bind in a context where only web-search is appropriate. The combination of Shadow + Constrained is a clean operator customisation model.

---

### `doc-pipeline`

**Depth:** Medium. Three design concepts, subsections with stage table and code callout, three paragraphs of background.

**What this shows:**

- **Pattern B (skills-only)** — `PromptRegistry::new()` with no `prompts/` directory. Skills carry all domain logic and format instructions; no `system.j2` or `react.j2` is loaded.
- **Self-directing skill chain** — Each non-terminal skill appends `NEXT_SKILL: <name>` as its last output line. `main.rs` strips the directive and routes to the named stage. The pipeline topology (extractor → analyzer → summarizer) lives entirely in SKILL.md files, not in Rust.
- **Per-stage strategies** — Three agents share one SkillConfig and ToolRegistry; each is built with a different `StrategyKind` to match its task.

**How to run:**

| Variable | Required | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | Yes | Anthropic API key (`MODEL_API_KEY` also accepted) |
| `MODEL_NAME` | No | Model ID (default: `claude-sonnet-4-6`) |

```bash
ANTHROPIC_API_KEY=sk-ant-... \
MODEL_NAME=claude-sonnet-4-6 \
cargo run -p example-doc-pipeline -- "your document text here"
```

**How it's built:**

Three agents are created with `make_agent()` — same runner, tools, and prompts; each loads its own `SkillConfig` from the same `skills_dir`. They differ only in `StrategyKind`:

| Stage | Strategy | Tools used | Emits |
|---|---|---|---|
| `extractor` | React | `find_dates` | Timeline events + `NEXT_SKILL: analyzer` |
| `analyzer` | Plan | `count_mentions` | Entity counts + `NEXT_SKILL: summarizer` |
| `summarizer` | React | `word_count` | Final summary (≤150 words, no directive) |

All three agents share one `ToolRegistry` (FindDates, CountMentions, WordCount). The active skill's `agentverse.tools` list restricts which tools appear in each stage's context — extractor sees only `find_dates`, analyzer only `count_mentions`, etc.

The dispatch loop in `main.rs`:
1. `create_session_with_skill("user", current_skill)` — binds the stage skill explicitly.
2. `invoke(...)` — runs the stage.
3. `parse_next_skill(output)` — strips `NEXT_SKILL: <name>` from the last line if present.
4. If a directive was found: set `current_skill = next`, pass clean output as next input, loop.
5. If no directive: print the final output and stop.

A `HashSet<String>` tracks visited skills; revisiting a skill exits with a cycle error.

**Design background:**

The self-directing chain inverts control: instead of `main.rs` knowing the topology (`if stage == "extractor" then next = "analyzer"`), each skill declares its own successor. Adding a new stage means writing a new SKILL.md that emits `NEXT_SKILL: new-stage` from its predecessor and adding the new stage to the match arm in `main.rs`. Removing or reordering stages requires no Rust change at all.

The tradeoff is implicit topology: to understand the pipeline you must read each SKILL.md. Code-defined routing (as in support-router) is explicit but requires recompilation to change. For short, linear, well-understood pipelines the self-directing pattern is lower friction. For branching or multi-domain routing, coordinator dispatch is clearer.

Pattern B (no `prompts/` directory) was intentional: the skills carry everything and demonstrate that `system.j2` is optional.

---

### `support-router`

**Depth:** Medium. Three design concepts, subsections with agent table and context-threading callout, three paragraphs of background.

**What this shows:**

- **Coordinator dispatch** — A coordinator agent (React, no tools) reads the request and outputs a JSON routing plan `[{skill, task}, ...]`. `main.rs` parses the plan and dispatches each step to the specialist agent with the matching skill.
- **Mixed strategies per role** — Each agent role uses the strategy best suited to its task: coordinator=React (zero tools, one-shot), billing=Hierarchical (multi-step decomposition), tech-support and account-mgmt=React (single tool call each).
- **Context threading** — Each specialist receives the previous step's output prepended as `Context from previous steps:`, so later steps can reference earlier findings.

**How to run:**

| Variable | Required | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | Yes | Anthropic API key (`MODEL_API_KEY` also accepted) |
| `MODEL_NAME` | No | Model ID (default: `claude-sonnet-4-6`) |

```bash
ANTHROPIC_API_KEY=sk-ant-... \
MODEL_NAME=claude-sonnet-4-6 \
cargo run -p example-support-router -- "I was charged twice last month and my API is down"
```

**How it's built:**

Four agents are created with `make_agent()`:

| Role | Strategy | Tools | Skill |
|---|---|---|---|
| coordinator | React (0 tools) | — | `coordinator` (explicit bind) |
| billing | Hierarchical | LookupInvoice, CheckRefundEligibility | `billing` |
| tech-support | React | CheckServiceStatus | `tech-support` |
| account-mgmt | React | GetAccountDetails | `account-mgmt` |

Coordinator and specialist agents use separate `ToolRegistry` instances. Specialists share one registry with all four domain tools; the active skill's `agentverse.tools` list restricts which are visible per invocation.

Dispatch flow:
1. `coordinator_agent.invoke(...)` → raw JSON string (possibly wrapped in markdown fences).
2. `parse_plan()` strips fences, finds the first `[…]` array, deserialises into `Vec<PlanStep>`.
3. For each `PlanStep { skill, task }`: look up the specialist agent, build input (`task` + accumulated context), `create_session_with_skill("user", skill)`, `invoke`, append `[skill]\noutput` to context.

**Design background:**

Coordinator dispatch was chosen for multi-domain requests where a single message can require multiple independent specialists. A support request like "charged twice AND my API is down" requires billing and tech-support — a linear self-directing chain (doc-pipeline pattern) would need to hardcode that order. The coordinator sees the full request and decides dynamically which specialists to call and in what sequence.

The coordinator is a pure router: it has no domain knowledge and no tools. Its SKILL.md instructs it to output only JSON. React with zero tools becomes a one-shot LLM call — the agent reasons in a single step and emits the plan. This makes the coordinator fast and its behaviour easy to test.

Context threading makes later specialists aware of earlier results without the coordinator needing to predict dependencies. In production you would add a confirmation step before executing destructive actions (e.g., processing a refund), and you would handle partial plan failures (one specialist errors, others continue).

---

### `business-report`

**Depth:** Medium. Three design concepts, numbered build walkthrough with component diagram, two paragraphs of background.

**What this shows:**

- **LLM-driven multi-agent orchestration** — The `business-report` SKILL.md instructs the agent to call `spawn_subagent` three times (financial, market, timeline analysts). The orchestration logic — which subagents to spawn, what objective to give each, in what order — lives in the skill file, not in Rust.
- **SubAgentTool integration** — `SubAgentExecutor::register_tool` registers `spawn_subagent` in the agent's tool registry as a first-class ReAct tool. The main agent calls it like any other tool.
- **In-process MCP server** — Domain tools (MarketSizingCalculator, RunwayProjector, MilestoneScheduler, RiskAdjustedSchedule) are served via an in-process MCP server. Subagents discover and call them through the MCP client.

**How to run:**

| Variable | Required | Description |
|---|---|---|
| `MODEL_BASE_URL` | No | OpenAI-compatible API base URL (default: `http://localhost:9090/v1`) |
| `MODEL_API_KEY` | No | API key |
| `MODEL_NAME` | No | Model ID (default: `Qwen3.6-35B-A3B-GGUF`) |

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_API_KEY=<key> \
MODEL_NAME=<model> \
cargo run -p example-business-report -- "B2B SaaS data pipeline startup"
```

**How it's built:**

```
main.rs
  │
  ├─ 1. McpServer (in-process, random port)
  │     └─ domain tools: MarketSizingCalculator, RunwayProjector,
  │                       MilestoneScheduler, RiskAdjustedSchedule
  │
  ├─ 2. McpClient → discovers mcp_tools ToolRegistry
  │
  ├─ 3. SubAgentExecutor(cm, mcp_tools, prompts)
  │     └─ subagents receive mcp_tools
  │
  ├─ 4. agent_tools: spawn_subagent only
  │     └─ SubAgentExecutor::register_tool(&executor, &agent_tools)
  │
  └─ 5. Agent(React, agent_tools, business-report skill)
        └─ invoke("Generate a business report for: …")
              │
              ├─ spawn_subagent(financial-analyst, …)
              ├─ spawn_subagent(market-analyst, …)
              ├─ spawn_subagent(timeline-analyst, …)
              └─ Answer: synthesised report
```

The SKILL.md declares `agentverse.tools: [spawn_subagent]`, ensuring the agent sees only that one tool. The skill body contains the full orchestration instructions: which three analysts to spawn, what objective to give each, and how to synthesise their outputs into a structured report.

**Design background:**

This is the LLM-driven counterpart to `project-feasibility`. The core question was: should orchestration logic live in Rust or in the skill? Putting it in the skill has two advantages: the LLM can adapt based on intermediate results (e.g., skip the timeline analyst if the NPV is already strongly negative), and a non-engineer can change what analysts are spawned by editing SKILL.md. The tradeoff is reliability — an LLM can omit a step or hallucinate a tool call; Rust code cannot. For exploratory, adaptive pipelines, LLM-driven orchestration is more flexible. For fixed, predictable pipelines, use programmatic spawning (project-feasibility).

---

### `project-feasibility`

**Depth:** Long. Four design concepts, ASCII pipeline diagram, annotated code callouts for parallel spawn and ResourceContent, three paragraphs of background.

**What this shows:**

- **Programmatic multi-agent with SubAgentExecutor** — Three analyst subagents spawned directly in Rust via `executor.spawn()`. All three start immediately and run concurrently. No LLM orchestrates the spawning decision.
- **Parallel fan-out + sequential synthesis** — Spawning returns a `SubAgentHandle` immediately (non-blocking). All handles are collected before any `.await_result()` call, which is what achieves true parallelism. A sequential synthesis subagent reads all three results as `ResourceContent`.
- **ResourceContent** — Completed subagent outputs are wrapped in `ResourceContent { label, content }` and passed to the synthesis subagent's `SubAgentContext`. The synthesis agent reads all three analyst reports as context without additional tool calls.
- **Budget limits** — Each subagent has an explicit `Budget { max_steps, max_tokens, timeout }`. Budgets are not optional safety theater — they are load-control: each analyst receives exactly the tokens it needs and no more.

**How to run:**

| Variable | Required | Description |
|---|---|---|
| `MODEL_BASE_URL` | No | OpenAI-compatible API base URL (default: `http://localhost:9090/v1`) |
| `MODEL_API_KEY` | No | API key |
| `MODEL_NAME` | No | Model ID (default: `Qwen3.6-35B-A3B-GGUF`) |

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_API_KEY=<key> \
MODEL_NAME=<model> \
cargo run -p example-project-feasibility -- "AI-powered inventory management SaaS"
```

**How it's built:**

```
project input
    │
    ├─── financial-analyst (spawn) ── project_cost_estimator, npv_calculator
    │                                  budget: 8 steps / 4000 tokens / 90s
    ├─── timeline-analyst  (spawn) ── milestone_scheduler
    │                                  budget: 5 steps / 3000 tokens / 60s
    └─── risk-analyst      (spawn) ── risk_adjusted_schedule
                                       budget: 6 steps / 3000 tokens / 60s
    │
    └─── [await_result × 3] → Vec<ResourceContent>
    │
    └─── synthesis (run) ── reads all three ResourceContent as context
                             budget: 5 steps / 6000 tokens / 90s
    │
    └─── Feasibility Report → stdout
```

Key code points:

**Parallel spawn** — Storing all handles before awaiting is what makes the subagents run in parallel:
```rust
let labeled: Vec<(&str, SubAgentHandle)> = vec![
    ("Financial Analysis", executor.spawn(financial_spec, base_ctx.clone())),
    ("Timeline Analysis",  executor.spawn(timeline_spec,  base_ctx.clone())),
    ("Risk Analysis",      executor.spawn(risk_spec,      base_ctx.clone())),
];
// All three are running now. await_result() collects them:
for (label, handle) in labeled {
    match handle.await_result().await { ... }
}
```

**ResourceContent** — Passes analyst outputs to synthesis without a tool call:
```rust
resources.push(ResourceContent {
    label: label.to_string(),
    content: r.answer.clone(),
});
// ...
let synthesis_ctx = SubAgentContext { resources, depth: 0 };
executor.run(&synthesis_spec, synthesis_ctx).await
```

Domain tools are served via an in-process MCP server (same pattern as business-report). `SubAgentExecutor::new(cm, mcp_tools, prompts)` makes those tools available to all subagents. The `system_prompt` field in each `SubAgentSpec` is where safety rules for subagents belong — `SubAgentExecutor` calls `build_initial_messages` directly and does not render `system.j2`.

**Design background:**

Built to be the programmatic counterpart to `business-report`. The design decision was explicit: when the pipeline is fixed and well-defined — feasibility analysis always needs financial, timeline, and risk — hardcoding the topology in Rust gives you reliability guarantees that LLM orchestration cannot. The three analysts always run, always in parallel, always within their budgets. The LLM cannot skip a step or accidentally spawn a fourth analyst.

`ResourceContent` was chosen over tool-based result passing because the synthesis agent does not need to query the results interactively — it needs all three at once to write a coherent report. Injecting them as context is both simpler and cheaper (no extra tool-call round trips).

In production you would replace the in-process MCP server with a real external service, add retry logic to `await_result()` for transient failures, and consider a results-validation step before synthesis to catch analysts that returned low-quality or incomplete output.

---

## Implementation Notes

- File: `examples/<name>/README.md` for each of the 7 examples.
- No links to spec files. All background written inline.
- Code snippets use `rust` fencing. Shell commands use `bash` fencing.
- ASCII diagrams use plain text (no Unicode box-drawing characters).
- The env var table uses `|---|---|---|` alignment (three columns: name, required, description).
