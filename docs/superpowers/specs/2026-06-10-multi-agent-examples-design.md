# Multi-Agent Examples Design

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two new example crates demonstrating the subagent runtime — one programmatic (`project-feasibility`), one LLM-driven via the skill system (`business-report`) — backed by six domain-specific MCP tools in a shared `demo-tools` crate.

**Architecture:** Three new crates added to the workspace. `demo-tools` defines six self-contained computational tools (no external APIs). `project-feasibility` calls `SubAgentExecutor::run_many()` directly to fan out three analyst subagents in parallel, then runs a synthesis subagent on the collected results. `business-report` registers `SubAgentTool` into an Agent's tool registry and activates a `business-report` skill that instructs the LLM to orchestrate three analyst subagents.

**Tech Stack:** Rust, tokio, `agentverse-subagent`, `agentverse-mcp`, `agentverse-agent`, `agentverse-tools`, `agentverse-strategy`, `agentverse-session`, `agentverse-skill`, OpenAI-compatible local LLM provider.

---

## Crate Overview

| Crate | Path | Description |
|---|---|---|
| `agentverse-demo-tools` | `examples/demo-tools` | 6 domain tool implementations, no infrastructure |
| `example-project-feasibility` | `examples/project-feasibility` | Programmatic `run_many` pipeline demo |
| `example-business-report` | `examples/business-report` | LLM-driven skill + `spawn_subagent` demo |

---

## 1. `examples/demo-tools`

### Purpose

Provides six pure-computation tools that serve as realistic MCP-exposed capabilities for both examples. All tools are deterministic and require no external services.

### File structure

```
examples/demo-tools/
  Cargo.toml
  src/
    lib.rs
    project_cost_estimator.rs
    milestone_scheduler.rs
    market_sizing_calculator.rs
    runway_projector.rs
    npv_calculator.rs
    risk_adjusted_schedule.rs
```

### Tools

#### `ProjectCostEstimator` (`"project_cost_estimator"`)

Estimates development cost and return on investment.

Input schema:
```json
{
  "team_size": integer,
  "avg_monthly_salary_usd": number,
  "duration_months": integer,
  "overhead_pct": number,        // e.g. 0.3 for 30%
  "projected_revenue_year1_usd": number,
  "projected_revenue_year2_usd": number
}
```

Output: total cost, monthly burn rate, cumulative revenue at 24 months, net profit/loss, ROI %.

#### `MilestoneScheduler` (`"milestone_scheduler"`)

Projects phase start/end dates from a list of phases with durations.

Input schema:
```json
{
  "start_date": "YYYY-MM-DD",
  "phases": [
    { "name": string, "duration_weeks": integer, "depends_on": [string] }
  ]
}
```

Output: per-phase start date, end date, total project end date, critical path (longest dependency chain).

#### `MarketSizingCalculator` (`"market_sizing_calculator"`)

Calculates TAM / SAM / SOM.

Input schema:
```json
{
  "total_addressable_market_usd": number,
  "target_segment_pct": number,    // fraction of TAM reachable
  "capture_rate_pct": number,      // fraction of SAM to capture
  "years_to_som": integer
}
```

Output: TAM, SAM, SOM, implied annual revenue at full capture, monthly revenue target.

#### `RunwayProjector` (`"runway_projector"`)

Projects cash runway and break-even month.

Input schema:
```json
{
  "initial_funding_usd": number,
  "monthly_burn_usd": number,
  "monthly_revenue_usd": number,
  "monthly_revenue_growth_pct": number   // e.g. 0.1 for 10% MoM
}
```

Output: runway months, break-even month, cash at 12/18/24 months, series-A readiness signal (bool: runway > 12 months and break-even < 18 months).

#### `NpvCalculator` (`"npv_calculator"`)

Calculates NPV, approximate IRR, and payback period.

Input schema:
```json
{
  "initial_investment_usd": number,
  "annual_cash_flows_usd": [number],   // one entry per year
  "discount_rate_pct": number          // e.g. 0.1 for 10%
}
```

Output: NPV, approximate IRR (binary search), payback period in years, cumulative cash flow per year.

#### `RiskAdjustedSchedule` (`"risk_adjusted_schedule"`)

PERT-based schedule risk analysis.

Input schema:
```json
{
  "phases": [
    {
      "name": string,
      "optimistic_weeks": number,
      "likely_weeks": number,
      "pessimistic_weeks": number
    }
  ]
}
```

Output: per-phase expected duration `(O + 4M + P) / 6`, standard deviation, total expected duration, 80% confidence duration (mean + 0.84σ), 95% confidence duration (mean + 1.65σ).

---

## 2. `examples/project-feasibility`

### Purpose

Demonstrates programmatic subagent orchestration: three analyst subagents run in parallel via `SubAgentExecutor::run_many()`, then a synthesis subagent reads all three results as `ResourceContent` and produces a structured feasibility report with a PROCEED / HOLD / REJECT verdict.

### File structure

```
examples/project-feasibility/
  Cargo.toml
  src/main.rs
  prompts/react.j2
```

### Environment variables

```
MODEL_BASE_URL=http://localhost:9090/v1   # default
MODEL_API_KEY=                            # default empty
MODEL_NAME=Qwen3.6-35B-A3B-GGUF         # default
```

### `prompts/react.j2`

Format-only preamble. No persona, no instructions — those come from `SubAgentSpec.system_prompt`.

```jinja2
Available tools:
{{ tools }}

Respond using this format:

    Thought: <reasoning>
    Action: <tool_name>
    Action Input: <valid JSON matching the tool's input schema>

When you have a final answer:

    Thought: <summary of findings>
    Answer: <your complete answer>
```

### MCP server tools registered

`ProjectCostEstimator`, `NpvCalculator`, `MilestoneScheduler`, `RiskAdjustedSchedule`

### Subagent specs

All four subagents share the same `PromptRegistry` (loaded from `prompts/`) and the same `ConnectionManager`.

#### Stage 1 — parallel (`run_many`)

**financial-analyst**
- `system_prompt`: `"You are a financial analyst. Use project_cost_estimator to estimate total development cost and npv_calculator to evaluate long-term return. Be specific with numbers."`
- `allowed_tools`: `["project_cost_estimator", "npv_calculator"]`
- `objective`: `"Estimate the total development cost, NPV, and 3-year ROI for the following project: {project_description}. Assume a 12% discount rate and project durations of 18 months."`
- `budget`: max_steps=8, max_tokens=4000, timeout=90s

**timeline-analyst**
- `system_prompt`: `"You are a project timeline analyst. Use milestone_scheduler to map delivery phases from today's date."`
- `allowed_tools`: `["milestone_scheduler"]`
- `objective`: `"Project a realistic delivery timeline with key phases for: {project_description}. Start from today. Include phases: Discovery, MVP, Beta, GA."`
- `budget`: max_steps=5, max_tokens=3000, timeout=60s

**risk-analyst**
- `system_prompt`: `"You are a risk analyst. Use risk_adjusted_schedule to quantify schedule uncertainty. Identify the top 5 technical and business risks."`
- `allowed_tools`: `["risk_adjusted_schedule"]`
- `objective`: `"Identify the top 5 risks for the following project and quantify schedule risk using PERT estimates: {project_description}."`
- `budget`: max_steps=6, max_tokens=3000, timeout=60s

#### Stage 2 — sequential (`run`)

**synthesis**
- `system_prompt`: `"You are a senior consultant. You synthesize multi-domain analyses into a clear executive report with an actionable recommendation."`
- `allowed_tools`: `[]`
- `resources`: three `ResourceContent` entries built from Stage 1 results:
  - `{ label: "Financial Analysis", content: <financial-analyst answer> }`
  - `{ label: "Timeline Analysis", content: <timeline-analyst answer> }`
  - `{ label: "Risk Analysis", content: <risk-analyst answer> }`
- `objective`: `"Based on the financial, timeline, and risk analyses provided in the context, write a structured Project Feasibility Report with sections: Executive Summary, Financial Outlook, Delivery Timeline, Risk Profile, and a final verdict: PROCEED / HOLD / REJECT with justification."`
- `budget`: max_steps=5, max_tokens=6000, timeout=90s

### `main.rs` structure

```
1. Parse CLI arg: project_description
2. Start McpServer with [ProjectCostEstimator, NpvCalculator,
                         MilestoneScheduler, RiskAdjustedSchedule]
3. Connect McpClient → populate mcp_tools registry via McpCatalogSource
4. Build ConnectionManager::openai(base_url, model, key)
5. Build PromptRegistry from prompts/
6. Build SubAgentExecutor::new(cm, mcp_tools, prompts)
7. Build 3 SubAgentSpec + SubAgentContext (depth=0) structs
8. executor.run_many(tasks).await → Vec<Result>
9. Collect results; print per-analyst summary; build ResourceContent vec
10. Build synthesis SubAgentSpec + ctx with 3 resources
11. executor.run(&synthesis_spec, synthesis_ctx).await
12. Print final report
13. Exit
```

### Run

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
cargo run -p example-project-feasibility -- \
  "A real-time collaborative code editor with AI suggestions"
```

---

## 3. `examples/business-report`

### Purpose

Demonstrates LLM-driven subagent orchestration: a `business-report` skill activates when the user submits a company or product description, and instructs the main agent to spawn three analyst subagents via the `spawn_subagent` tool. Whether the three calls execute in a single turn (parallel) or sequentially depends on the local model's multi-tool-call behaviour; the code path handles both cases correctly.

### File structure

```
examples/business-report/
  Cargo.toml
  src/main.rs
  prompts/react.j2
  prompts/system.j2
  skills/system/business-report/SKILL.md
```

### Environment variables

Same as project-feasibility.

### `prompts/react.j2`

Same minimal format-only template as project-feasibility.

### `prompts/system.j2`

```
You are a business intelligence orchestrator. You coordinate specialist
subagents to produce comprehensive, multi-domain business analyses.
```

### `skills/system/business-report/SKILL.md`

```markdown
---
name: business-report
description: >
  Produces a multi-domain business report by spawning specialist analyst subagents.
  Use when the user asks for a business report, company analysis, or product assessment.
version: 1.0.0
agentverse:
  tools:
    - spawn_subagent
---

# Business Report

You produce business reports by orchestrating three specialist subagents in sequence,
then synthesizing their findings.

## Analysts to spawn

For each analyst, call spawn_subagent with the parameters below, substituting
the user's company/product description for `{subject}`.

### 1. market-analyst

```json
{
  "name": "market-analyst",
  "objective": "Assess the market opportunity for {subject}. Use market_sizing_calculator to size TAM/SAM/SOM. Estimate realistic capture rates and time to reach SOM.",
  "system_prompt": "You are a market research analyst. Use market_sizing_calculator to quantify the market opportunity.",
  "allowed_tools": ["market_sizing_calculator"],
  "max_steps": 6,
  "max_tokens": 3000,
  "timeout_secs": 60
}
```

### 2. financial-analyst

```json
{
  "name": "financial-analyst",
  "objective": "Project the financial trajectory for {subject}. Use runway_projector to model cash runway and break-even given realistic funding, burn, and revenue growth assumptions.",
  "system_prompt": "You are a financial analyst. Use runway_projector to model cash runway and break-even timing.",
  "allowed_tools": ["runway_projector"],
  "max_steps": 6,
  "max_tokens": 3000,
  "timeout_secs": 60
}
```

### 3. operations-analyst

```json
{
  "name": "operations-analyst",
  "objective": "Map the operational build-out plan for {subject}. Use milestone_scheduler to project phases and risk_adjusted_schedule to quantify delivery uncertainty.",
  "system_prompt": "You are an operations analyst. Use milestone_scheduler and risk_adjusted_schedule to plan and de-risk the build-out.",
  "allowed_tools": ["milestone_scheduler", "risk_adjusted_schedule"],
  "max_steps": 8,
  "max_tokens": 4000,
  "timeout_secs": 90
}
```

## Synthesis

After all three analysts have responded, write a **Business Report** with these sections:

1. **Executive Summary** — 3-sentence overview
2. **Market Opportunity** — from market-analyst findings
3. **Financial Outlook** — from financial-analyst findings
4. **Operations Plan** — from operations-analyst findings
5. **Recommendation** — INVEST / MONITOR / PASS with one-paragraph justification
```

### MCP server tools registered

`MarketSizingCalculator`, `RunwayProjector`, `MilestoneScheduler`, `RiskAdjustedSchedule`

### `main.rs` structure

```
1. Parse CLI arg: subject (company/product description)
2. Start McpServer with [MarketSizingCalculator, RunwayProjector,
                         MilestoneScheduler, RiskAdjustedSchedule]
3. Connect McpClient → populate mcp_tools registry via McpCatalogSource
4. Build ConnectionManager::openai(base_url, model, key)
5. Build PromptRegistry from prompts/
6. Build SubAgentExecutor::new(cm, Arc::clone(&mcp_tools), Arc::clone(&prompts))
   (SubAgentExecutor holds mcp_tools internally; subagents receive them via filter_by_names)
7. Build agent_tools = ToolRegistry::new()
   SubAgentExecutor::register_tool(&executor, &agent_tools)
   (agent only needs spawn_subagent; domain tools are accessed by subagents, not the main agent)
8. Build LlmRunner::from_config(ProviderConfig::OpenAI { ... })
9. Load SkillConfig::load(skills_dir, SkillMode::Constrained(["business-report"]))
10. Build Agent with React strategy + skills
11. Create session → agent.invoke(session_id, question)
12. Print final report
13. Exit
```

### Run

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
cargo run -p example-business-report -- \
  "A SaaS platform for restaurant inventory management"
```

---

## 4. Workspace changes

Add to `Cargo.toml` `[workspace] members`:
```toml
"examples/demo-tools",
"examples/project-feasibility",
"examples/business-report",
```

`examples/demo-tools/Cargo.toml` dependencies: `agentverse` (path), `async-trait` (workspace), `schemars` (workspace), `serde` (workspace), `serde_json` (workspace), `chrono` (workspace).

Both example `Cargo.toml` files depend on `agentverse-demo-tools = { path = "../demo-tools" }` plus the standard agent/mcp/subagent/strategy/session crates.

---

## 5. Prompt approach rationale

| Layer | Old approach (mcp-demo) | New approach |
|---|---|---|
| `react.j2` | Mixed format + persona instructions | Format-only: tools list + Thought/Action/Answer syntax |
| Subagent persona | N/A | `SubAgentSpec.system_prompt` — set per-subagent in Rust |
| Main agent persona | All in `react.j2` | `prompts/system.j2` — base identity |
| Workflow instructions | All in `react.j2` | `SKILL.md` body — activated by skill router |

This separation means `react.j2` never needs to change between projects; behaviour lives in the skill and spec layers where it belongs.

---

## 6. Error handling

- `run_many` results are `Vec<Result<SubAgentResult, SubAgentError>>`. The pipeline prints a warning for any failed analyst and uses `"[analysis unavailable]"` as the resource content so the synthesis subagent still runs.
- MCP server failure is fatal (`.expect()`); both examples are single-shot demos, not long-running services.
- The synthesis subagent receiving degraded input produces a partial report noting which analyses were unavailable.

---

## 7. Testing

Both examples are integration tests by nature — they require a running local LLM. No unit tests are added for the example binaries themselves.

`demo-tools` gets unit tests for each tool's computation logic: given known inputs, assert the numeric outputs are correct. This validates the tool implementations without needing an LLM.
