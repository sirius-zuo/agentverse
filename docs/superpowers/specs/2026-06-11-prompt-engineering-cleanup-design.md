# Prompt Engineering Cleanup Design

**Date:** 2026-06-11
**Status:** Approved

## Overview

The examples were written incrementally — some before the skill system existed, others after. The result is inconsistent: `system.j2` files in some examples duplicate content that lives in SKILL.md; `react.j2` is loaded in `project-feasibility` by a `SubAgentExecutor` that never reads it; `doc-pipeline` and `support-router` use a different pattern (skills-only, no `prompts/` dir) with no explanation of why. This cleanup establishes two named patterns, applies them consistently, and updates the reference documentation.

---

## The Two Patterns

### Pattern A — Prompts-Primary

Used when an agent has a `prompts/` directory.

**`system.j2`** — cross-skill baseline only. Permitted content:
- One sentence of agent identity (what kind of agent this is at the broadest level)
- Behavioral invariants that apply regardless of skill: output language, factuality, response style
- Safety rules: do not fabricate data, decline harmful requests

Prohibited content: domain logic, workflow steps, tool guidance, output formats specific to one skill. The test: *if the instruction would change when switching skills, it belongs in SKILL.md, not system.j2.*

**Strategy template** (`react.j2`, `hierarchical.j2`, `plan_and_execute.j2`) — kept as-is. Format instructions for the chosen strategy are legitimately cross-skill and belong here.

**SKILL.md** — authoritative source for everything domain-specific: skill persona, workflow, tool guidance, output format requirements.

### Pattern B — Skills-Only

Used when the agent's behavior is entirely defined by skills and no cross-skill baseline is needed.

`PromptRegistry::new()` with no `prompts/` directory. SKILL.md carries all instructions including any format guidance the LLM needs. Demonstrates that `system.j2` is optional.

`doc-pipeline` and `support-router` stay as-is to document this pattern.

---

## Decision Rule

> If the instruction would change when switching skills → SKILL.md.
> If it applies regardless of which skill is active → system.j2.

---

## Per-Example Changes

### `hello-agent`

**Pattern A.** Currently `system.j2` includes "designed to help with simple tasks and answer basic questions" — task scope is per-skill.

`system.j2` → `"You are a helpful assistant. Be concise, accurate, and honest."`

No other changes. `react.j2` is correctly a format-only preamble.

### `business-report`

**Pattern A.** Currently `system.j2` says "You are a business intelligence orchestrator. You coordinate specialist subagents…" — exact duplicate of the SKILL.md opening.

`system.j2` → `"You are an analytical agent. Respond with well-structured, factual outputs. Do not fabricate figures or data."`

No other changes. `react.j2` is correctly a format-only preamble.

### `code-review-agent`

**Pattern A.** Currently `system.j2` contains a full 5-step code review workflow — exact duplicate of SKILL.md content.

`system.j2` → `"You are a software engineering agent. Be thorough, precise, and constructive. Do not fabricate file contents or code."`

No other changes. `hierarchical.j2` is correctly a format-only template.

### `web-search-agent`

**Pattern A.** Currently `system.j2` includes tool guidance ("Use the web_search tool…") and citation rules — both already in SKILL.md.

`system.j2` → `"You are a research agent. Be accurate and cite your sources. Do not invent information."`

No other changes. `plan_and_execute.j2` is correctly a format-only template.

### `project-feasibility`

**Pattern A.** Currently has `react.j2` loaded via `PromptRegistry::from_config` but uses `SubAgentExecutor` at the top level — `SubAgentExecutor` calls `build_initial_messages` directly and never reads `react.j2`. The template is dead weight.

Changes:
- Delete `prompts/react.j2`
- Create `prompts/system.j2` with a minimal baseline (the file serves as a documentation anchor; note that `SubAgentExecutor` does not render it at the parent level — safety rules intended for subagents should be included in each `SubAgentSpec.system_prompt`)
- Change `PromptRegistry::from_config(...)` to `PromptRegistry::new()` in `main.rs`

`system.j2` → `"You are an analytical research agent. Be precise and factual. Do not fabricate data or sources."`

### `doc-pipeline` and `support-router`

**Pattern B.** No changes. These examples demonstrate that `system.j2` is optional and skills can carry the full prompt.

---

## Documentation Changes

### `DEVELOPMENT.md` — Prompt Engineering section

Rewrite the section to lead with the two named patterns:
- Pattern A vs Pattern B decision criteria
- The `system.j2` content contract (permitted / prohibited)
- Annotated example of a thin `system.j2` (3–5 lines)
- Note that strategy templates are cross-skill format layers — unchanged
- Note that `doc-pipeline` / `support-router` are deliberate Pattern B examples
- Existing strategy template directory layouts and example file docs remain

### `README.md` — Prompt Templates section

Extend the existing 12-line section to name the two patterns and give a 2-sentence description of each, so readers know which to reach for before consulting DEVELOPMENT.md.

---

## What Does Not Change

- All SKILL.md files — content is correct; domain logic is already in the right layer
- All strategy templates (`react.j2`, `hierarchical.j2`, `plan_and_execute.j2`) — format instructions are cross-skill, correct as-is
- `doc-pipeline` and `support-router` in their entirety
- The skill routing, explicit binding, shadow/extend patterns — out of scope
