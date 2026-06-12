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
