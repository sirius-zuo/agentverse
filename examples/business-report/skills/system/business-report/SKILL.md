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

You produce business reports by orchestrating three specialist subagents, then
synthesizing their findings into a structured report.

## Step 1 — spawn three analysts

Call `spawn_subagent` three times (sequentially or in one turn if your LLM
supports parallel tool calls), substituting the user's subject for `{subject}`:

**market-analyst**
```json
{
  "name": "market-analyst",
  "objective": "Assess the market opportunity for {subject}. Use market_sizing_calculator to size TAM/SAM/SOM. Estimate realistic capture rates and time to reach SOM.",
  "system_prompt": "You are a market research analyst. Use market_sizing_calculator to quantify the market opportunity with specific numbers.",
  "allowed_tools": ["market_sizing_calculator"],
  "max_steps": 6,
  "max_tokens": 3000,
  "timeout_secs": 60
}
```

**financial-analyst**
```json
{
  "name": "financial-analyst",
  "objective": "Project the financial trajectory for {subject}. Use runway_projector to model cash runway and break-even given realistic funding, burn, and revenue growth assumptions.",
  "system_prompt": "You are a financial analyst. Use runway_projector to model cash runway and break-even timing with specific numbers.",
  "allowed_tools": ["runway_projector"],
  "max_steps": 6,
  "max_tokens": 3000,
  "timeout_secs": 60
}
```

**operations-analyst**
```json
{
  "name": "operations-analyst",
  "objective": "Map the operational build-out plan for {subject}. Use milestone_scheduler to project phases from today and risk_adjusted_schedule to quantify delivery uncertainty.",
  "system_prompt": "You are an operations analyst. Use milestone_scheduler and risk_adjusted_schedule to plan and de-risk the build-out.",
  "allowed_tools": ["milestone_scheduler", "risk_adjusted_schedule"],
  "max_steps": 8,
  "max_tokens": 4000,
  "timeout_secs": 90
}
```

## Step 2 — synthesize

After all three analysts have responded, write a **Business Report** with:

1. **Executive Summary** — 3-sentence overview
2. **Market Opportunity** — from market-analyst findings
3. **Financial Outlook** — from financial-analyst findings
4. **Operations Plan** — from operations-analyst findings
5. **Recommendation** — INVEST / MONITOR / PASS with one-paragraph justification
