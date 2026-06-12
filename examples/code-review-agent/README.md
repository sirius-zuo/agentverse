# code-review-agent

A code-review agent that demonstrates explicit skill binding and per-session tool restriction via SKILL.md.

## What this shows

**Explicit skill binding** — `create_session_with_skill("user", "code-review")` binds the skill before the first message. `SkillRouter` never runs. The session is locked to `code-review` from creation, making behaviour deterministic regardless of how the user phrases their request.

**Tool restriction via SKILL.md** — The `code-review` skill declares `agentverse.tools: [file_search, shell]`. Only those two tools appear in the agent's preamble during this session. All other tools in the registry are invisible to the LLM.

## How to run

| Variable | Required | Description |
|---|---|---|
| `MODEL_BASE_URL` | No | OpenAI-compatible API base URL (default: `http://localhost:9090/v1`) |
| `MODEL_NAME` | No | Model ID (default: `Qwen3.6-35B-A3B-GGUF`) |
| `MODEL_API_KEY` | No | API key (default: empty) |
| `PROJECT_DIR` | Yes | Directory `ShellTool` uses as its working directory |

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
PROJECT_DIR=/path/to/your/project \
cargo run -p example-code-review-agent
```

Type a review request and press Enter (e.g., `"Review the authentication module for security issues"`). Type `exit` to quit.

## How it's built

`FileSearch` and `ShellTool` are registered in `ToolRegistry`. `ShellTool` is scoped to `PROJECT_DIR` with a 30-second timeout. Destructive commands (`rm`, `rmdir`, `mv`, `dd`, `sudo`, `chmod`, `chown`) are blocked at the tool level. Note: `workdir` is not a filesystem sandbox — absolute paths and symlinks can escape. For production, run inside a container or seccomp sandbox.

The agent uses a Hierarchical strategy (max 10 steps): it decomposes the review request into sub-goals (security, performance, style, logic) and executes each as a plan step. This strategy is well-suited for reviews because each area can be investigated independently.

`SkillConfig::load(skills_dir, SkillMode::Open)` loads the skill catalogue, but routing never runs. `create_session_with_skill("user", "code-review")` locks the skill at session creation. The `code-review` SKILL.md's `agentverse.tools: [file_search, shell]` list then filters `active_tool_names` before each preamble render.

## Design background

Built to show the "you already know which skill you need" case. When a user launches a code-review agent, the intent is unambiguous — routing adds latency for no benefit and can fail on ambiguous first messages. Explicit binding removes that variable entirely.

Tool restriction demonstrates the minimal-privilege benefit of SKILL.md: the agent cannot reach tools that would be irrelevant or distracting for code review. In production you would also run the agent inside an isolated environment and add a rate limit on `ShellTool` invocations.
