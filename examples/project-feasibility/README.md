# project-feasibility

A feasibility analysis pipeline that demonstrates programmatic subagent fan-out, ResourceContent for result passing, and Budget limits for load control.

## What this shows

**Programmatic subagent fan-out** — Three analyst subagents are spawned directly in Rust via `executor.spawn()`. All three start immediately and run concurrently. No LLM decides what to spawn or when.

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

Built as the programmatic counterpart to `business-report`. The core question: when should the subagent topology be hardcoded in Rust vs driven by the LLM? When the pipeline is fixed and well-defined — feasibility analysis always needs financial, timeline, and risk — hardcoded topology gives reliability guarantees that LLM orchestration cannot: the three analysts always run, always in parallel, always within their budgets. The LLM cannot skip a step or spawn a fourth analyst.

`ResourceContent` was chosen over tool-based result passing because the synthesis agent needs all three reports simultaneously to write a coherent analysis. Injecting them as context at creation time is simpler and cheaper than having the synthesis agent request each report via a tool call.

In production you would replace the in-process MCP server with an external service, add retry logic to `await_result()` for transient failures, and validate analyst output quality before passing it to synthesis.
