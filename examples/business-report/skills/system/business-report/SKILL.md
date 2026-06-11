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

You have access to `spawn_subagent`. Using the user's company or product description
as the subject, call it three times:

**market-analyst** — spawn with these parameters:

- `name`: `"market-analyst"`
- `objective`: write an objective that asks the analyst to assess the market opportunity
  for the user's specific company/product by sizing TAM, SAM, and SOM via
  `market_sizing_calculator` with realistic capture rates and expected time to reach SOM
- `system_prompt`: `"You are a market research analyst. Use market_sizing_calculator to quantify the market opportunity with specific numbers."`
- `allowed_tools`: `["market_sizing_calculator"]`
- `max_steps`: 6, `max_tokens`: 8000, `timeout_secs`: 60

**financial-analyst** — spawn with these parameters:

- `name`: `"financial-analyst"`
- `objective`: write an objective that asks the analyst to project the financial trajectory
  for the user's specific company/product by modeling cash runway and break-even via
  `runway_projector` with realistic funding, burn, and revenue growth assumptions
- `system_prompt`: `"You are a financial analyst. Use runway_projector to model cash runway and break-even timing with specific numbers."`
- `allowed_tools`: `["runway_projector"]`
- `max_steps`: 6, `max_tokens`: 8000, `timeout_secs`: 60

**operations-analyst** — spawn with these parameters:

- `name`: `"operations-analyst"`
- `objective`: write an objective that asks the analyst to map the operational build-out
  plan for the user's specific company/product by projecting phases from today via
  `milestone_scheduler` and quantifying delivery uncertainty via `risk_adjusted_schedule`
- `system_prompt`: `"You are an operations analyst. Use milestone_scheduler and risk_adjusted_schedule to plan and de-risk the build-out."`
- `allowed_tools`: `["milestone_scheduler", "risk_adjusted_schedule"]`
- `max_steps`: 8, `max_tokens`: 10000, `timeout_secs`: 90

## Step 2 — synthesize

After all three analysts have responded, write a **Business Report** with:

1. **Executive Summary** — 3-sentence overview
2. **Market Opportunity** — from market-analyst findings
3. **Financial Outlook** — from financial-analyst findings
4. **Operations Plan** — from operations-analyst findings
5. **Recommendation** — INVEST / MONITOR / PASS with one-paragraph justification
