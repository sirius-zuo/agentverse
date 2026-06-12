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
