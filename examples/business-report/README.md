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
