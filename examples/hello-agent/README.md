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

## Optional: long-term memory (dev wiring)

Local dev uses an OpenAI-compatible local embedder (e.g. Ollama) + LanceDB:

```rust
use agentverse_memory::{EmbedderRegistry, VectorLongtermMemory};
use agentverse_memory_lancedb::LanceDbVectorStore;
use std::{collections::HashMap, sync::Arc};

let embedder = EmbedderRegistry::with_builtins().build("openai", &HashMap::from([
    ("model_name".into(), "nomic-embed-text".into()),
    ("base_url".into(), "http://localhost:11434/v1".into()), // Ollama, no api_key
    ("dimensions".into(), "768".into()),
]))?;
let store = Arc::new(LanceDbVectorStore::new("./data/lancedb", "memories", 768));
let longterm = Arc::new(VectorLongtermMemory::new(embedder, store));
// Agent::builder(...).with_longterm_memory(longterm).build();
```

Production: swap `"openai"`+key (or `"gemini"`) and `agentverse_memory_pgvector::PgVectorStore`.
