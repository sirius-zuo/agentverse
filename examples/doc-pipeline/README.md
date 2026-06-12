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
