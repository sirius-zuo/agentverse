# Prompt Management System Design

## Problem

Only `ReActStrategy` uses the `PromptRegistry` for templating. `PlanStrategy`, `HierarchicalStrategy`, and `StrategyRouter` all use hardcoded `format!()` strings. The `Agent::invoke()` method creates a `PromptRegistry` but never uses it. There is no system prompt concept, no few-shot examples, and no way to configure prompts.

## Design Decisions

1. **No runtime versioning** — Git provides rollback, diff, and tags. Internal versioning is overhead without a clear runtime need.
2. **No hot-swap** — Runtime prompt swapping is unnecessary flexibility that adds attack surface.
3. **Hybrid approach** — Default templates embedded in code (always available), optional `prompts/` directory for overrides at runtime.
4. **Minijinja** — Already a dependency, fast compiled templates, Jinja2-compatible syntax.
5. **File formats** — `.j2` for templates (clear signal), `.toml` for examples (structured data).
6. **Config-driven** — Examples defined in YAML/JSON config files that get loaded into `PromptRegistry` alongside templates. Examples and templates co-located, versioned together via Git.

## Architecture

```
Config (prompts section)
    → PromptRegistry::from_config() [loads defaults + user overrides]
    → PromptRegistry.render(category, context) → rendered string
    → Compose system + strategy prompts
    → prompt string → LLM
```

## Data Model

### Config Structure

```yaml
model_api_key: "..."
model_name: "..."
max_messages: 100

prompts_dir: "prompts/"           # optional
system_prompt: "..."              # optional inline override

prompts:
  strategies:
    react:
      template: "..."
      examples: "react_examples"
    
    plan_and_execute:
      template: "..."
      examples: "plan_examples"
    
    hierarchical:
      template: "..."
      examples: "hierarchical_examples"
  
  router:
    template: "..."
    examples: "router_examples"
  
  examples:
    react_examples:
      - input: "..."
        output: "..."
    
    plan_examples:
      - input: "..."
        output: "..."
    
    router_examples:
      - input: "..."
        strategy: "react"
```

### Rust Data Model

```rust
pub struct Example {
    pub input: String,
    pub output: Option<String>,   // used by strategy examples
    pub strategy: Option<String>, // used by router examples
}

pub struct PromptRegistry {
    env: Environment<'static>,
    examples: HashMap<String, Vec<Example>>,
}
```

### Config Integration

`Config` in `avs-core` gets new fields:

```rust
pub struct Config {
    pub model_api_key: String,
    pub model_name: String,
    pub max_messages: usize,
    #[serde(default)]
    pub tools: Vec<String>,
    
    #[serde(default)]
    pub prompts_dir: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}
```

`AgentBuilder` gets fluent APIs:

```rust
let agent = Agent::builder()
    .config(config)
    .system_prompt("You are a sarcastic assistant.")
    .prompt_dir("custom-prompts/")
    .build()?;
```

## Prompt Categories

| Category | Purpose | Example Content |
|----------|---------|-----------------|
| `system` | Agent identity, constraints, tone | "You are a helpful AI assistant..." |
| `strategies.react` | ReAct pattern instructions + format | "Think step by step. Use Thought/Action/Answer format." |
| `strategies.plan_and_execute` | Planning format instructions | "Generate a JSON plan with id, description, tool..." |
| `strategies.plan_and_execute.synthesis` | Final answer synthesis | "Synthesize results into a final answer." |
| `strategies.hierarchical` | Hierarchical planning format | "Decompose into sub-goals, plan each, synthesize." |
| `strategies.hierarchical.decompose` | Sub-goal decomposition | "Break into sub-goals, plan each, synthesize." |
| `router` | Strategy selection instructions | "Choose the best strategy from: react, plan_and_execute..." |

## Composition

At runtime, the final prompt is composed:

```
Final prompt = system_prompt + "\n\n" + strategy_prompt + context_variables
```

The system prompt is shared across all strategies. The strategy prompt is selected based on which strategy is running. Both are rendered with the same context (conversation, tools, examples).

## PromptRegistry API

```rust
impl PromptRegistry {
    // Load defaults + optional directory + config overrides
    pub fn from_config(config: &PromptConfig) -> Result<Self, AgentError>;
    
    // Register a template by name
    pub fn add_template(&mut self, name: &str, template: &str);
    
    // Register example sets
    pub fn add_examples(&mut self, name: &str, examples: Vec<Example>);
    
    // Render a template by name with context
    pub fn render(&self, name: &str, context: HashMap<String, Value>) -> Result<String, AgentError>;
    
    // Get examples for a category
    fn get_examples_for_category(&self, category: &str) -> Option<&[Example]>;
}
```

## File Loading

### Directory Structure

```
prompts/
  react.j2                  # Jinja2 template
  plan_and_execute.j2
  hierarchical.j2
  router.j2
  system.j2
  react_examples.toml       # Example set
  plan_examples.toml
  router_examples.toml
```

### Loading Logic

1. Load default embedded templates (react, plan_and_execute, hierarchical, router, system)
2. If `prompts_dir` exists, load all `.j2` files (keyed by filename without extension)
3. Load all `.toml` files as example sets (keyed by filename without extension)
4. Config YAML/JSON overrides can also register templates and examples

### File Format

**`.j2` template** (plain text, no special format):
```jinja2
You are using the ReAct pattern: Think → Act → Observe.

Available tools:
{{ tools }}

{% if examples %}
Here are some examples:
{% for example in examples %}
User: {{ example.input }}
Assistant: {{ example.output }}
{% endfor %}
{% endif %}

Respond in this format:
Thought: [your reasoning]
Action: [tool name]
Action Input: [tool arguments as JSON]

Or if you have the final answer:
Thought: [your reasoning]
Answer: [final answer]
```

**`.toml` example file**:
```toml
[[example]]
input = "What is the weather in Tokyo?"
output = "Thought: I need to check the weather.\nAction: weather\nAction Input: {\"city\": \"Tokyo\"}"

[[example]]
input = "What is 2 + 3?"
output = "Thought: I can calculate this.\nAction: calculator\nAction Input: {\"expression\": \"2 + 3\"}"
```

**Router examples** (note `strategy` field instead of `output`):
```toml
[[example]]
input = "What time is it?"
strategy = "react"

[[example]]
input = "Plan a trip to Paris including flights and hotels"
strategy = "hierarchical"
```

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Missing `prompts_dir` | Skip file loading, use defaults (no error) |
| Malformed `.j2` | Return error (template won't compile) |
| Malformed `.toml` | Return error (can't parse examples) |
| File not found | Return error (directory exists but missing expected files) |

## Crate Changes

| Crate | Changes |
|-------|---------|
| `avs-core` | Enhanced `PromptRegistry` (file loading, examples), new `Example` struct, `Config` gets `prompts_dir` and `system_prompt` fields, `AgentBuilder` gets prompt APIs |
| `avs-react` | `CycleSkeleton` composes system + strategy prompts |
| `avs-plan` | Replace hardcoded strings with templated prompts via `PromptRegistry` |
| `avs-router` | Replace hardcoded prompt with templated prompt via `PromptRegistry` |

## Default Embedded Templates

### `system` (default)
```
You are a helpful AI assistant that executes tasks using available tools.
You are concise and accurate. Never claim to have done something you haven't.
If you don't know something, say so.
```

### `strategies.react` (default)
```
You are using the ReAct pattern: Think → Act → Observe.

Available tools:
{{ tools }}

{% if examples %}
Here are some examples:
{% for example in examples %}
User: {{ example.input }}
Assistant: {{ example.output }}
{% endfor %}
{% endif %}

Respond in this format:
Thought: [your reasoning]
Action: [tool name]
Action Input: [tool arguments as JSON]

Or if you have the final answer:
Thought: [your reasoning]
Answer: [final answer]
```

### `strategies.plan_and_execute` (default)
```
You are a planning assistant. Generate a step-by-step plan.

Available tools:
{{ tools }}

{% if examples %}
Examples:
{% for example in examples %}
User: {{ example.input }}
Assistant: {{ example.output }}
{% endfor %}
{% endif %}

Respond with ONLY a JSON object:
{"description": "...", "steps": [{"id": 1, "description": "...", "tool": "...", "args": {}, "depends_on": []}]}
```

### `strategies.hierarchical.decompose` (default)
```
Break this request into sub-goals. Each sub-goal should be independently executable.

{% if examples %}
Examples:
{% for example in examples %}
Input: {{ example.input }}
Strategy: {{ example.strategy }}
{% endfor %}
{% endif %}

Respond with ONLY a JSON array of strings.
```

### `router` (default)
```
Choose the best orchestration strategy for this request.

Available strategies:
- react: simple Q&A, tool use, step-by-step reasoning
- plan_and_execute: tasks with clear upfront steps
- hierarchical: complex tasks needing decomposition

{% if examples %}
Examples:
{% for example in examples %}
Input: {{ example.input }}
Strategy: {{ example.strategy }}
{% endfor %}
{% endif %}

Respond with ONLY the strategy name.
```

## Guardrails Integration

Guardrails are placed at three boundaries in the prompt flow:

```
User Input → check_prompt (lightweight) → PromptRegistry → Rendered Prompt → check_prompt (real guardrail) → LLM → check_output → Response → Memory/Return
```

**Two guardrail checkpoints:**

1. **Pre-render** (`check_prompt` on raw user input) — catches obvious injection before entering the system. Light-weight, fast path.
2. **Post-render** (`check_prompt` on full rendered prompt) — catches injection that may have slipped through in conversation context (user messages, tool outputs, system prompt). This is the real guardrail.
3. **Post-LLM** (`check_output` on LLM response) — catches data leakage (SSN, emails, secrets) before response is stored or returned.

Guardrails live in the **strategy layer** (not `PromptRegistry`), because:
- `PromptRegistry` is pure templating — shouldn't know about security
- Strategies control the full prompt lifecycle — they call `render()`, then guardrail, then LLM, then guardrail

**API in `CycleSkeleton` (avs-react):**

```rust
pub async fn build_prompt_with_guardrails(&self) -> Result<String, AgentError> {
    let prompt = self.build_prompt()?;
    check_prompt(&prompt).map_err(|e| AgentError::Guardrail(e))?;
    Ok(prompt)
}
```

**API in strategy `run()` loop:**

```rust
let prompt = skeleton.build_prompt_with_guardrails()?;
let response = model.generate(&prompt, tool_defs).await?;
check_output(&response).map_err(|e| AgentError::Guardrail(e))?;
```

**New error variant:** `AgentError::Guardrail(GuardrailError)` — clean separation from model/tool/config errors.

**Config integration:** Guardrails enabled/disabled per strategy:

```yaml
strategies:
  react:
    guardrails:
      prompt_check: true    # check_prompt on rendered prompt
      output_check: true    # check_output on LLM response
    template: "..."
    examples: "react_examples"
```

## Example Agents Update

Current state: all 5 example agents call `Agent::invoke()` which is a placeholder returning `"Processed: ..."`.

After this design, each agent gets:
- A `prompts/` directory with custom `.j2` and `.toml` files
- Strategy instantiation (not placeholder invoke)
- Guardrails wired in
- Tool wiring where applicable

**Agent mapping:**

| Agent | Strategy | Tools | Guardrails |
|-------|----------|-------|------------|
| `hello-agent` | `ReActStrategy` | Calculator, DateTime | Enabled |
| `slack-hr-assistant` | `PlanStrategy` | SlackAdapter | Enabled |
| `rag-qa` | `ReActStrategy` | FileSearch, HttpClient | Enabled |
| `web-search-agent` | `ReActStrategy` | HttpClient | Enabled |
| `code-review-agent` | `HierarchicalStrategy` | FileSearch, HttpClient | Enabled |

Each agent loads prompts from its own `prompts/` directory (e.g., `examples/hello-agent/prompts/`).

## Implementation Plan

1. Add `toml` crate dependency to `avs-core`
2. Add `Example` struct and enhance `PromptRegistry` with file loading
3. Update `Config` to include `prompts_dir` and `system_prompt`
4. Update `AgentBuilder` with prompt APIs
5. Add `AgentError::Guardrail` variant
6. Wire `PromptRegistry` into `CycleSkeleton` (avs-react)
7. Add guardrail checkpoints to `CycleSkeleton::build_prompt_with_guardrails()`
8. Wire guardrails into strategy `run()` loops
9. Replace hardcoded strings in `PlanStrategy` (avs-plan)
10. Replace hardcoded strings in `HierarchicalStrategy` (avs-plan)
11. Replace hardcoded strings in `StrategyRouter` (avs-router)
12. Update `Agent::invoke()` to use the full prompt system
13. Update all 5 example agents with prompt architecture + guardrails + tool wiring
14. Add integration tests for prompt rendering with examples
15. Create example `.j2` and `.toml` files in each example agent's `prompts/` directory
16. Create default `.j2` and `.toml` files in project root `prompts/` directory

## Scope Check

This design is focused on:
- Prompt management infrastructure (registry, loading, examples)
- Wiring templates into existing strategies
- System prompt separation

This does NOT include:
- Prompt caching
- Prompt analytics/monitoring
- Multi-language prompt support
- Prompt A/B testing
- Dynamic prompt generation from config

The design is scoped for a single implementation plan.
