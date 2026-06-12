# Example READMEs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `README.md` to each of the 7 target examples so readers immediately understand what the example demonstrates, how to run it, and why it was built.

**Architecture:** One `README.md` per example, four fixed sections (What this shows / How to run / How it's built / Design background), depth scaled to example complexity. No links to spec files — all background written inline. Three short examples (hello-agent, code-review-agent, web-search-agent), three medium (doc-pipeline, support-router, business-report), one long (project-feasibility).

**Tech Stack:** Markdown only.

---

## File Map

| File | Action |
|---|---|
| `examples/hello-agent/README.md` | Create |
| `examples/code-review-agent/README.md` | Create |
| `examples/web-search-agent/README.md` | Create |
| `examples/doc-pipeline/README.md` | Create |
| `examples/support-router/README.md` | Create |
| `examples/business-report/README.md` | Create |
| `examples/project-feasibility/README.md` | Create |

---

## Task 1: `hello-agent/README.md`

**Files:**
- Create: `examples/hello-agent/README.md`

- [ ] **Step 1: Write the file**

Write `examples/hello-agent/README.md`:

```markdown
# hello-agent

A general-purpose interactive agent that demonstrates automatic skill routing and the Extend pattern for operator-added skills.

## What this shows

**SkillMode::Open + automatic routing** — Skills are discovered from `skills/system/` (math-helper, datetime-helper) and `skills/user/` (travel-advisor). On the first `invoke`, `SkillRouter` scores all candidates against the user message and binds the best match for the session's lifetime. If no skill scores high enough, the agent responds using all skill summaries as soft context without binding.

**Extend pattern** — A new skill dropped into `skills/user/` is immediately available with no code change. This mirrors how an operator extends a shipped agent at deployment time — `system/` ships with the binary, `user/` is added without recompilation.

## How to run

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

Type a message and press Enter. Type `exit` or press Ctrl+C to quit.

## How it's built

`ToolRegistry` registers `Calculator` (category: `math`) and `DateTimeTool` (category: `utility`). `PromptRegistry::from_config` loads `prompts/` — a thin `system.j2` (cross-skill identity only) and `react.j2` (ReAct format instructions).

`SkillConfig::load(skills_dir, SkillMode::Open)` walks `skills/system/` then `skills/user/`, building a skill catalogue from all `SKILL.md` files found. The catalogue holds descriptions and tool allowlists for each skill.

`Agent::new(...)` with a ReAct strategy (max 10 steps). `create_session("user")` creates the session but does not run routing yet. The router fires on the first `invoke` call: it scores each skill's `description` field against the user message and binds the highest-scoring match. Subsequent calls in the same session use the bound skill without re-routing.

To try the Extend pattern: add a new directory under `skills/user/`, write a `SKILL.md` with `name`, `description`, and an `agentverse.tools` list, and restart — the new skill is immediately eligible for routing.

## Design background

This is the baseline example — the simplest complete agent, built to show the routing-to-binding lifecycle in its most transparent form. Using `SkillMode::Open` makes routing visible: ask a math question and `math-helper` binds; ask about travel and `travel-advisor` binds. The Extend pattern (user/ tier) demonstrates the layered directory model: `system/` is the vendor layer, `user/` is the operator layer.

In production you would replace `sqlite::memory:` with a persistent database path, add authentication to the REPL loop, and handle session recovery across restarts.
```

- [ ] **Step 2: Verify required sections present**

```bash
grep -c "## What this shows\|## How to run\|## How it's built\|## Design background" \
  examples/hello-agent/README.md
```
Expected: `4`

- [ ] **Step 3: Commit**

```bash
git add examples/hello-agent/README.md
git commit -m "docs(hello-agent): add README"
```

---

## Task 2: `code-review-agent/README.md`

**Files:**
- Create: `examples/code-review-agent/README.md`

- [ ] **Step 1: Write the file**

Write `examples/code-review-agent/README.md`:

```markdown
# code-review-agent

A code-review agent that demonstrates explicit skill binding and per-session tool restriction via SKILL.md.

## What this shows

**Explicit skill binding** — `create_session_with_skill("user", "code-review")` binds the skill before the first message. `SkillRouter` never runs. The session is locked to `code-review` from creation, making behaviour deterministic regardless of how the user phrases their request.

**Tool restriction via SKILL.md** — The `code-review` skill declares `agentverse.tools: [file_search, shell]`. Only those two tools appear in the agent's preamble during this session. All other tools in the registry are invisible to the LLM.

## How to run

| Variable | Required | Description |
|---|---|---|
| `MODEL_BASE_URL` | No | OpenAI-compatible API base URL (default: `http://localhost:9090/v1`) |
| `MODEL_NAME` | No | Model ID (default: `Qwen3.6-35B-A3B-GGUF`) |
| `MODEL_API_KEY` | No | API key (default: empty) |
| `PROJECT_DIR` | Yes | Directory `ShellTool` uses as its working directory |

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
PROJECT_DIR=/path/to/your/project \
cargo run -p example-code-review-agent
```

Type a review request and press Enter (e.g., `"Review the authentication module for security issues"`). Type `exit` to quit.

## How it's built

`FileSearch` and `ShellTool` are registered in `ToolRegistry`. `ShellTool` is scoped to `PROJECT_DIR` with a 30-second timeout. Destructive commands (`rm`, `rmdir`, `mv`, `dd`, `sudo`, `chmod`, `chown`) are blocked at the tool level. Note: `workdir` is not a filesystem sandbox — absolute paths and symlinks can escape. For production, run inside a container or seccomp sandbox.

The agent uses a Hierarchical strategy (max 10 steps): it decomposes the review request into sub-goals (security, performance, style, logic) and executes each as a plan step. This strategy is well-suited for reviews because each area can be investigated independently.

`SkillConfig::load(skills_dir, SkillMode::Open)` loads the skill catalogue, but routing never runs. `create_session_with_skill("user", "code-review")` locks the skill at session creation. The `code-review` SKILL.md's `agentverse.tools: [file_search, shell]` list then filters `active_tool_names` before each preamble render.

## Design background

Built to show the "you already know which skill you need" case. When a user launches a code-review agent, the intent is unambiguous — routing adds latency for no benefit and can fail on ambiguous first messages. Explicit binding removes that variable entirely.

Tool restriction demonstrates the minimal-privilege benefit of SKILL.md: the agent cannot reach tools that would be irrelevant or distracting for code review. In production you would also run the agent inside an isolated environment and add a rate limit on `ShellTool` invocations.
```

- [ ] **Step 2: Verify required sections present**

```bash
grep -c "## What this shows\|## How to run\|## How it's built\|## Design background" \
  examples/code-review-agent/README.md
```
Expected: `4`

- [ ] **Step 3: Commit**

```bash
git add examples/code-review-agent/README.md
git commit -m "docs(code-review-agent): add README"
```

---

## Task 3: `web-search-agent/README.md`

**Files:**
- Create: `examples/web-search-agent/README.md`

- [ ] **Step 1: Write the file**

Write `examples/web-search-agent/README.md`:

```markdown
# web-search-agent

A web-search agent that demonstrates constrained skill routing and the Shadow pattern for operator-level skill overrides.

## What this shows

**SkillMode::Constrained** — `SkillMode::Constrained(vec!["web-search"])` makes only skills named `web-search` eligible for routing. Any other skills in `skills/` are invisible to the router, regardless of how well they match the user message.

**Shadow pattern** — `skills/user/web-search/` declares the same `name: web-search` as `skills/system/web-search/`. When `SkillConfig::load` walks `skills/system/` then `skills/user/`, the user variant (v1.1.0) silently replaces the system variant (v1.0.0). The result: stricter citation rules activate with no code change.

## How to run

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

## How it's built

`WebSearch` tool registered in `ToolRegistry`. The agent uses a Plan strategy (max 5 steps): plan which pages to fetch, execute the fetches, synthesise results.

`SkillConfig::load` walks `skills/system/` first, then `skills/user/`. When both directories contain a skill with the same `name` field (`web-search`), the user/ entry overwrites the system/ entry in the catalogue. `SkillMode::Constrained(vec!["web-search"])` then ensures only this skill is eligible — even if an operator adds unrelated skills to `user/`, they cannot accidentally bind.

`create_session("user")` runs the router. With exactly one eligible skill whose description matches any search-related message, binding is deterministic. The agent receives the user/ variant of the skill, which requires numbered footnote citations in its output.

## Design background

The Shadow pattern was designed for deployment-time customisation without forking. The base agent ships with a permissive `web-search` skill in `system/`. A deployment that needs stricter citation rules drops an override into `user/` — no recompilation, no code change, no fork.

`SkillMode::Constrained` complements Shadow by making the restriction explicit in code: even in an open catalogue, this agent type only ever does web search. The combination — Shadow for customisation, Constrained for restriction — is a clean operator customisation model.
```

- [ ] **Step 2: Verify required sections present**

```bash
grep -c "## What this shows\|## How to run\|## How it's built\|## Design background" \
  examples/web-search-agent/README.md
```
Expected: `4`

- [ ] **Step 3: Commit**

```bash
git add examples/web-search-agent/README.md
git commit -m "docs(web-search-agent): add README"
```

---

## Task 4: `doc-pipeline/README.md`

**Files:**
- Create: `examples/doc-pipeline/README.md`

- [ ] **Step 1: Write the file**

Write `examples/doc-pipeline/README.md`:

```markdown
# doc-pipeline

A three-stage document analysis pipeline that demonstrates self-directing skill chains and per-stage strategy selection.

## What this shows

**Pattern B (skills-only)** — `PromptRegistry::new()` with no `prompts/` directory. Skills carry all domain logic and format instructions. No `system.j2` or `react.j2` is loaded, demonstrating that the prompts layer is optional.

**Self-directing skill chain** — Each non-terminal skill appends `NEXT_SKILL: <name>` as its last output line. `main.rs` strips the directive and routes to the named stage. The pipeline topology lives entirely in SKILL.md files — not in Rust.

**Per-stage strategies** — Three agents share a runner and tool registry but are built with different `StrategyKind` values to match each stage's task type.

## How to run

| Variable | Required | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | Yes | Anthropic API key (`MODEL_API_KEY` also accepted) |
| `MODEL_NAME` | No | Model ID (default: `claude-sonnet-4-6`) |

```bash
ANTHROPIC_API_KEY=sk-ant-... \
MODEL_NAME=claude-sonnet-4-6 \
cargo run -p example-doc-pipeline -- "your document text here"
```

## How it's built

Three agents are created with `make_agent()` — same runner, tools, and prompts; each loads its own `SkillConfig` from the same `skills_dir`. They differ only in `StrategyKind`:

| Stage | Strategy | Tools | Emits |
|---|---|---|---|
| `extractor` | React | `find_dates` | Timeline events + `NEXT_SKILL: analyzer` |
| `analyzer` | Plan | `count_mentions` | Entity counts + `NEXT_SKILL: summarizer` |
| `summarizer` | React | `word_count` | Final summary (no directive) |

All three agents share one `ToolRegistry` (FindDates, CountMentions, WordCount). Each stage's active skill restricts which tools appear in its context via its `agentverse.tools` list — extractor sees only `find_dates`, analyzer only `count_mentions`, summarizer only `word_count`.

The dispatch loop in `main.rs`:
1. `create_session_with_skill("user", current_skill)` — binds the stage skill explicitly.
2. `invoke(...)` — runs the stage agent.
3. `parse_next_skill(output)` — strips `NEXT_SKILL: <name>` from the last line if present.
4. If a directive was found: set `current_skill = next`, pass clean output as next input, loop.
5. If no directive: print the final output and stop.

A `HashSet<String>` tracks visited skills; a repeated skill name exits with a cycle error.

## Design background

The self-directing chain inverts control: each skill declares its own successor rather than `main.rs` encoding the pipeline topology. Adding a new stage means writing a new SKILL.md (and one match arm in `main.rs`). Reordering or removing stages requires only SKILL.md edits — no Rust recompilation.

The tradeoff is implicit topology: to understand the full pipeline you must read each SKILL.md. For short, linear, well-understood pipelines this is acceptable. For branching or multi-domain routing, coordinator dispatch (see `support-router`) is more explicit and easier to reason about.

Pattern B (no `prompts/` directory) is intentional: this example shows that skills can carry everything the LLM needs without a shared system baseline.
```

- [ ] **Step 2: Verify required sections present**

```bash
grep -c "## What this shows\|## How to run\|## How it's built\|## Design background" \
  examples/doc-pipeline/README.md
```
Expected: `4`

- [ ] **Step 3: Commit**

```bash
git add examples/doc-pipeline/README.md
git commit -m "docs(doc-pipeline): add README"
```

---

## Task 5: `support-router/README.md`

**Files:**
- Create: `examples/support-router/README.md`

- [ ] **Step 1: Write the file**

Write `examples/support-router/README.md`:

```markdown
# support-router

A multi-domain support agent that demonstrates coordinator dispatch: a coordinator agent produces a structured routing plan, and specialist agents execute each step.

## What this shows

**Coordinator dispatch** — A coordinator agent (React, no tools) reads the support request and outputs a JSON plan `[{skill, task}, ...]`. `main.rs` parses the plan and dispatches each step to the specialist agent that owns the matching skill.

**Mixed strategies per role** — Each agent uses the strategy suited to its task: coordinator=React (zero tools, one-shot JSON), billing=Hierarchical (multi-step decomposition), tech-support and account-mgmt=React (single tool call each).

**Context threading** — Each specialist receives the previous step's output prepended as `Context from previous steps:`. Later specialists can reference earlier findings without the coordinator predicting dependencies upfront.

## How to run

| Variable | Required | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | Yes | Anthropic API key (`MODEL_API_KEY` also accepted) |
| `MODEL_NAME` | No | Model ID (default: `claude-sonnet-4-6`) |

```bash
ANTHROPIC_API_KEY=sk-ant-... \
MODEL_NAME=claude-sonnet-4-6 \
cargo run -p example-support-router -- "I was charged twice last month and my API is down"
```

## How it's built

Four agents are created with `make_agent()`:

| Role | Strategy | Tools | Skill |
|---|---|---|---|
| coordinator | React (0 tools) | — | `coordinator` (explicit bind) |
| billing | Hierarchical | LookupInvoice, CheckRefundEligibility | `billing` |
| tech-support | React | CheckServiceStatus | `tech-support` |
| account-mgmt | React | GetAccountDetails | `account-mgmt` |

Coordinator and specialist agents use separate `ToolRegistry` instances. Specialists share one registry with all four domain tools; each skill's `agentverse.tools` list restricts which are visible per session.

Dispatch flow:
1. `coordinator_agent.invoke(...)` → raw string (may be wrapped in markdown fences).
2. `parse_plan()` strips fences, finds the first `[...]` array, deserialises into `Vec<PlanStep { skill, task }>`.
3. For each step: resolve the specialist agent by skill name, build input (`task` + accumulated context from all previous steps), `create_session_with_skill("user", skill)`, `invoke`, append `[skill]\noutput` to context.

The coordinator uses React with zero registered tools, which collapses to a single reasoning-and-output step rather than a tool-call loop — a fast, cheap one-shot LLM call whose only job is routing.

## Design background

Coordinator dispatch was designed for requests that span multiple independent domains simultaneously. A request like "charged twice AND my API is down" requires both billing and tech-support — a linear self-directing chain (see `doc-pipeline`) cannot express this without hardcoding the combination. The coordinator sees the full request and decides dynamically which specialists to invoke and in what order.

The coordinator is a pure router: no domain knowledge, no tools, no side effects. Keeping it domain-ignorant means its output is easy to validate in isolation: given a request string, does it produce a valid JSON routing plan?

Context threading makes later specialists aware of earlier results without the coordinator predicting dependencies. In production you would add confirmation before destructive actions (e.g., processing a refund), handle partial plan failures gracefully, and add a per-specialist timeout.
```

- [ ] **Step 2: Verify required sections present**

```bash
grep -c "## What this shows\|## How to run\|## How it's built\|## Design background" \
  examples/support-router/README.md
```
Expected: `4`

- [ ] **Step 3: Commit**

```bash
git add examples/support-router/README.md
git commit -m "docs(support-router): add README"
```

---

## Task 6: `business-report/README.md`

**Files:**
- Create: `examples/business-report/README.md`

- [ ] **Step 1: Write the file**

Write `examples/business-report/README.md`:

```markdown
# business-report

A multi-agent report generator that demonstrates LLM-driven subagent orchestration: a skill instructs the agent to spawn specialist analyst subagents via tool calls.

## What this shows

**LLM-driven multi-agent orchestration** — The `business-report` SKILL.md instructs the agent to call `spawn_subagent` three times (financial, market, timeline analysts). The orchestration logic — which analysts to spawn, what objective to give each, and how to synthesise results — lives in the skill file, not in Rust.

**SubAgentTool integration** — `SubAgentExecutor::register_tool` registers `spawn_subagent` in the agent's tool registry as a first-class ReAct tool. From the agent's perspective, spawning a subagent is identical to calling any other tool.

**In-process MCP server** — Domain tools (MarketSizingCalculator, RunwayProjector, MilestoneScheduler, RiskAdjustedSchedule) are served via an in-process MCP server. Subagents discover and call them through the MCP client without any special wiring in `main.rs`.

## How to run

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

## How it's built

```
main.rs
  |
  +-- 1. McpServer (in-process, random port)
  |       domain tools: MarketSizingCalculator, RunwayProjector,
  |                     MilestoneScheduler, RiskAdjustedSchedule
  |
  +-- 2. McpClient -> discovers mcp_tools ToolRegistry
  |
  +-- 3. SubAgentExecutor(cm, mcp_tools, prompts)
  |       subagents receive mcp_tools
  |
  +-- 4. agent_tools: spawn_subagent only
  |       SubAgentExecutor::register_tool(&executor, &agent_tools)
  |
  +-- 5. Agent(React, agent_tools, business-report skill)
          invoke("Generate a business report for: ...")
            |
            +-- spawn_subagent(financial-analyst, ...)
            +-- spawn_subagent(market-analyst, ...)
            +-- spawn_subagent(timeline-analyst, ...)
            +-- Answer: synthesised report
```

The SKILL.md declares `agentverse.tools: [spawn_subagent]`, so the main agent sees only that one tool. The skill body contains the full orchestration instructions: which three analysts to spawn, what objective to give each, and how to synthesise their outputs into a structured report.

## Design background

This is the LLM-driven counterpart to `project-feasibility`. The key design question: should orchestration logic live in Rust or in the skill? Putting it in the skill has two advantages: the LLM can adapt the subagent sequence based on intermediate results (e.g., skip the timeline analyst if the NPV analysis is strongly negative), and a non-engineer can change what analysts are spawned by editing SKILL.md without recompiling.

The tradeoff is reliability — an LLM can omit a step or misname a tool argument; Rust code cannot. For fixed, predictable pipelines with known topology, use programmatic spawning (`project-feasibility`). For exploratory or adaptive workflows where the LLM should decide what to investigate next, this pattern provides more flexibility.
```

- [ ] **Step 2: Verify required sections present**

```bash
grep -c "## What this shows\|## How to run\|## How it's built\|## Design background" \
  examples/business-report/README.md
```
Expected: `4`

- [ ] **Step 3: Commit**

```bash
git add examples/business-report/README.md
git commit -m "docs(business-report): add README"
```

---

## Task 7: `project-feasibility/README.md`

**Files:**
- Create: `examples/project-feasibility/README.md`

- [ ] **Step 1: Write the file**

Write `examples/project-feasibility/README.md`:

```markdown
# project-feasibility

A programmatic multi-agent feasibility analysis pipeline that demonstrates parallel subagent fan-out, ResourceContent for result passing, and Budget limits for load control.

## What this shows

**Programmatic multi-agent with SubAgentExecutor** — Three analyst subagents are spawned directly in Rust via `executor.spawn()`. All three start immediately and run concurrently. No LLM decides what to spawn or when.

**Parallel fan-out + sequential synthesis** — `executor.spawn()` returns a `SubAgentHandle` immediately (non-blocking). All three handles are collected before any `.await_result()` call — this is what achieves true parallelism. A synthesis subagent then runs sequentially once all three complete.

**ResourceContent** — Completed subagent outputs are wrapped in `ResourceContent { label, content }` and passed to the synthesis subagent's `SubAgentContext`. The synthesis agent reads all three analyst reports as context at the start, with no tool calls required.

**Budget limits** — Each subagent has an explicit `Budget { max_steps, max_tokens, timeout }`. Budgets are load-control: each analyst gets exactly the capacity it needs and the pipeline cannot run away.

## How to run

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

## How it's built

```
project input
    |
    +--- financial-analyst (spawn) -- project_cost_estimator, npv_calculator
    |                                  budget: 8 steps / 4000 tokens / 90s
    +--- timeline-analyst  (spawn) -- milestone_scheduler
    |                                  budget: 5 steps / 3000 tokens / 60s
    +--- risk-analyst      (spawn) -- risk_adjusted_schedule
                                       budget: 6 steps / 3000 tokens / 60s
    |
    +--- [await_result x 3] -> Vec<ResourceContent>
    |
    +--- synthesis (run) -- reads all three ResourceContent as context
                             budget: 5 steps / 6000 tokens / 90s
    |
    +--- Feasibility Report -> stdout
```

Domain tools (ProjectCostEstimator, NpvCalculator, MilestoneScheduler, RiskAdjustedSchedule) are registered in a `ToolRegistry`, served via an in-process `McpServer` on a random port, then discovered by `McpClient` into `mcp_tools`. `SubAgentExecutor::new(cm, mcp_tools, prompts)` makes those tools available to all subagents.

**Parallel spawn** — storing all handles before awaiting is what makes the subagents run in parallel:

```rust
let labeled: Vec<(&str, SubAgentHandle)> = vec![
    ("Financial Analysis", executor.spawn(financial_spec, base_ctx.clone())),
    ("Timeline Analysis",  executor.spawn(timeline_spec,  base_ctx.clone())),
    ("Risk Analysis",      executor.spawn(risk_spec,      base_ctx.clone())),
];
// All three are running now. Collect results:
for (label, handle) in labeled {
    match handle.await_result().await { ... }
}
```

**ResourceContent** — passes analyst outputs to synthesis without additional tool calls:

```rust
resources.push(ResourceContent { label: label.to_string(), content: r.answer.clone() });
// ...
let synthesis_ctx = SubAgentContext { resources, depth: 0 };
executor.run(&synthesis_spec, synthesis_ctx).await
```

Each `SubAgentSpec` carries a `system_prompt` field — this is where safety rules for subagents belong. `SubAgentExecutor` calls `build_initial_messages` directly and does not render `system.j2` at the parent level.

## Design background

Built as the programmatic counterpart to `business-report`. The core question: when should the multi-agent topology be hardcoded in Rust vs driven by the LLM? When the pipeline is fixed and well-defined — feasibility analysis always needs financial, timeline, and risk — hardcoded topology gives reliability guarantees that LLM orchestration cannot: the three analysts always run, always in parallel, always within their budgets. The LLM cannot skip a step or spawn a fourth analyst.

`ResourceContent` was chosen over tool-based result passing because the synthesis agent needs all three reports simultaneously to write a coherent analysis. Injecting them as context at creation time is simpler and cheaper than having the synthesis agent request each report via a tool call.

In production you would replace the in-process MCP server with an external service, add retry logic to `await_result()` for transient failures, and validate analyst output quality before passing it to synthesis.
```

- [ ] **Step 2: Verify required sections present**

```bash
grep -c "## What this shows\|## How to run\|## How it's built\|## Design background" \
  examples/project-feasibility/README.md
```
Expected: `4`

- [ ] **Step 3: Commit**

```bash
git add examples/project-feasibility/README.md
git commit -m "docs(project-feasibility): add README"
```

---

## Task 8: Final check and push

- [ ] **Step 1: Verify all 7 READMEs exist**

```bash
for e in hello-agent code-review-agent web-search-agent doc-pipeline \
          support-router business-report project-feasibility; do
  echo "$e: $(wc -l < examples/$e/README.md) lines"
done
```
Expected: each example prints a line count above 0.

- [ ] **Step 2: Confirm no spec links snuck in**

```bash
grep -r "docs/superpowers/specs" examples/*/README.md
```
Expected: no output.

- [ ] **Step 3: Push**

```bash
GIT_SSH_COMMAND="ssh -i ~/.ssh/id_personal_git" git push
```
