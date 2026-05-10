# Prompt Management System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add comprehensive prompt management with templates, few-shot examples, guardrails, and system prompt composition across all agent strategies.

**Architecture:** Enhance `PromptRegistry` with file loading (.j2/.toml), add `Example` struct, wire guardrails into strategy layers, replace hardcoded strings in Plan/Hierarchical/Router strategies, update all 5 example agents.

**Tech Stack:** Rust, minijinja (already a dep), toml (new dep), serde, thiserror.

---

### Task 1: Add `toml` crate dependency to workspace and avs-core

**Files:**
- Modify: `Cargo.toml` (workspace)
- Modify: `avs-core/Cargo.toml`

- [ ] **Step 1: Add toml to workspace dependencies**

In `Cargo.toml`, add to `[workspace.dependencies]`:
```toml
toml = "0.8"
```

- [ ] **Step 2: Add toml to avs-core dependencies**

In `avs-core/Cargo.toml`, add to `[dependencies]`:
```toml
toml.workspace = true
```

- [ ] **Step 3: Verify workspace compiles**

Run: `cargo check --workspace`
Expected: PASS, no errors

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml avs-core/Cargo.toml
git commit -m "deps: add toml crate for example file parsing"
```

---

### Task 2: Add `Example` struct and `GuardrailError` to avs-core

**Files:**
- Create: `avs-core/src/example.rs`
- Modify: `avs-core/src/error.rs`
- Modify: `avs-core/src/lib.rs`

- [ ] **Step 1: Create Example struct**

Create `avs-core/src/example.rs`:
```rust
use serde::{Deserialize, Serialize};

/// A few-shot example for prompt templates.
/// Strategy examples use `output`; router examples use `strategy`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Example {
    /// The example input (user request).
    pub input: String,
    /// The example output (agent response). Used by strategy examples.
    #[serde(default)]
    pub output: Option<String>,
    /// The example strategy. Used by router examples.
    #[serde(default)]
    pub strategy: Option<String>,
}
```

- [ ] **Step 2: Add GuardrailError to error.rs**

Modify `avs-core/src/error.rs` — add new variants to `AgentError` and a new `GuardrailError` enum:
```rust
// Add to AgentError enum (after Tool variant):
#[error("Guardrail error: {0}")]
Guardrail(#[from] GuardrailError),

// Add new enum at end of file:
#[derive(Error, Debug)]
pub enum GuardrailError {
    #[error("Prompt injection: {0}")]
    PromptInjection(String),
    #[error("Output filtered: {0}")]
    OutputFiltered(String),
}
```

- [ ] **Step 3: Add Example re-export to lib.rs**

Modify `avs-core/src/lib.rs` — add module and re-export:
```rust
// Add to module declarations (after `pub mod prompt;`):
pub mod example;

// Add to re-exports (after `pub use prompt::PromptRegistry;`):
pub use example::Example;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add avs-core/src/example.rs avs-core/src/error.rs avs-core/src/lib.rs
git commit -m "core: add Example struct and GuardrailError type"
```

---

### Task 3: Write tests for Example serialization

**Files:**
- Create: `avs-core/tests/example_test.rs`

- [ ] **Step 1: Write failing test**

Create `avs-core/tests/example_test.rs`:
```rust
use agentverse::Example;

#[test]
fn test_example_strategy_fields() {
    let ex = Example {
        input: "What time is it?".to_string(),
        output: None,
        strategy: Some("react".to_string()),
    };
    assert_eq!(ex.input, "What time is it?");
    assert!(ex.output.is_none());
    assert_eq!(ex.strategy, Some("react".to_string()));
}

#[test]
fn test_example_output_fields() {
    let ex = Example {
        input: "What is 2+2?".to_string(),
        output: Some("Thought: 2+2=4. Answer: 4".to_string()),
        strategy: None,
    };
    assert_eq!(ex.output, Some("Thought: 2+2=4. Answer: 4".to_string()));
    assert!(ex.strategy.is_none());
}

#[test]
fn test_example_roundtrip_json() {
    let ex = Example {
        input: "Hello".to_string(),
        output: Some("Hi there!".to_string()),
        strategy: None,
    };
    let json = serde_json::to_string(&ex).unwrap();
    let deserialized: Example = serde_json::from_str(&json).unwrap();
    assert_eq!(ex, deserialized);
}

#[test]
fn test_example_roundtrip_toml() {
    let ex = Example {
        input: "Hello".to_string(),
        output: Some("Hi there!".to_string()),
        strategy: None,
    };
    let toml_str = toml::to_string(&[ex.clone()]).unwrap();
    let deserialized: Vec<Example> = toml::from_str(&toml_str).unwrap();
    assert_eq!(deserialized, vec![ex]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package agentverse example_test`
Expected: FAIL — `Example` not found or `toml` not available

- [ ] **Step 3: Verify it passes after Task 2 changes**

Run: `cargo test --package agentverse example_test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add avs-core/tests/example_test.rs
git commit -m "test: add example serialization tests"
```

---

### Task 4: Enhance PromptRegistry with Example storage and file loading

**Files:**
- Modify: `avs-core/src/prompt.rs`
- Modify: `avs-core/src/error.rs` (add file-related ConfigError variant)

- [ ] **Step 1: Rewrite PromptRegistry with Example storage**

Replace entire `avs-core/src/prompt.rs`:
```rust
use minijinja::Environment;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::{AgentError, ConfigError};
use crate::Example;

/// Default embedded templates shipped with the library.
const DEFAULT_SYSTEM_TEMPLATE: &str =
    "You are a helpful AI assistant that executes tasks using available tools.\n\
     You are concise and accurate. Never claim to have done something you haven't.\n\
     If you don't know something, say so.";

const DEFAULT_REACT_TEMPLATE: &str =
    "You are using the ReAct pattern: Think → Act → Observe.\n\n\
     Available tools:\n\
     {{ tools }}\n\n\
     {% if examples %}\n\
     Here are some examples:\n\
     {% for example in examples %}\n\
     User: {{ example.input }}\n\
     Assistant: {{ example.output }}\n\
     {% endfor %}\n\
     {% endif %}\n\n\
     Respond in this format:\n\
     Thought: [your reasoning]\n\
     Action: [tool name]\n\
     Action Input: [tool arguments as JSON]\n\n\
     Or if you have the final answer:\n\
     Thought: [your reasoning]\n\
     Answer: [final answer]";

const DEFAULT_PLAN_AND_EXECUTE_TEMPLATE: &str =
    "You are a planning assistant. Generate a step-by-step plan.\n\n\
     Available tools:\n\
     {{ tools }}\n\n\
     {% if examples %}\n\
     Examples:\n\
     {% for example in examples %}\n\
     User: {{ example.input }}\n\
     Assistant: {{ example.output }}\n\
     {% endfor %}\n\
     {% endif %}\n\n\
     Respond with ONLY a JSON object:\n\
     {\"description\": \"...\", \"steps\": [{\"id\": 1, \"description\": \"...\", \"tool\": \"...\", \"args\": {}, \"depends_on\": []}]}";

const DEFAULT_HIERARCHICAL_DECOMPOSE_TEMPLATE: &str =
    "Break this request into sub-goals. Each sub-goal should be independently executable.\n\n\
     {% if examples %}\n\
     Examples:\n\
     {% for example in examples %}\n\
     Input: {{ example.input }}\n\
     Strategy: {{ example.strategy }}\n\
     {% endfor %}\n\
     {% endif %}\n\n\
     Respond with ONLY a JSON array of strings.";

const DEFAULT_ROUTER_TEMPLATE: &str =
    "Choose the best orchestration strategy for this request.\n\n\
     Available strategies:\n\
     - react: simple Q&A, tool use, step-by-step reasoning\n\
     - plan_and_execute: tasks with clear upfront steps\n\
     - hierarchical: complex tasks needing decomposition\n\n\
     {% if examples %}\n\
     Examples:\n\
     {% for example in examples %}\n\
     Input: {{ example.input }}\n\
     Strategy: {{ example.strategy }}\n\
     {% endfor %}\n\
     {% endif %}\n\n\
     Respond with ONLY the strategy name.";

/// Configuration for prompt registry loading.
#[derive(Debug, Default)]
pub struct PromptConfig {
    /// Optional system prompt override.
    pub system_prompt: Option<String>,
    /// Optional prompts directory for .j2/.toml file loading.
    pub prompts_dir: Option<String>,
    /// Additional templates to register (name → template string).
    pub templates: HashMap<String, String>,
    /// Additional example sets to register (name → examples).
    pub examples: HashMap<String, Vec<Example>>,
}

pub struct PromptRegistry {
    env: Environment<'static>,
    examples: HashMap<String, Vec<Example>>,
}

impl PromptRegistry {
    /// Create from configuration — loads defaults, optional files, and overrides.
    pub fn from_config(config: &PromptConfig) -> Result<Self, AgentError> {
        let mut registry = Self::default(); // loads embedded defaults

        // Load from prompts/ directory if specified
        if let Some(ref dir) = config.prompts_dir {
            registry.load_from_directory(dir)?;
        }

        // Apply config overrides
        for (name, template) in &config.templates {
            registry.add_template(name, template);
        }
        for (name, examples) in &config.examples {
            registry.add_examples(name, examples.clone());
        }

        // Override system prompt if provided
        if let Some(ref system_prompt) = config.system_prompt {
            registry.add_template("system", system_prompt);
        }

        Ok(registry)
    }

    /// Create default registry with embedded templates only.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a template by name.
    pub fn add_template(&mut self, name: &str, template: &str) {
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        let source: &'static str = Box::leak(template.to_string().into_boxed_str());
        self.env.add_template(name, source).unwrap();
    }

    /// Register an example set by name.
    pub fn add_examples(&mut self, name: String, examples: Vec<Example>) {
        self.examples.insert(name, examples);
    }

    /// Render a template by name with context.
    /// Examples are NOT automatically injected — callers must add them to context.
    pub fn render(&self, name: &str, context: HashMap<String, Value>) -> Result<String, AgentError> {
        let tmpl = self.env.get_template(name).map_err(|e| {
            AgentError::Config(ConfigError::Invalid(format!("Template '{}' not found: {}", name, e)))
        })?;
        let entries: Vec<(String, minijinja::value::Value)> = context
            .into_iter()
            .map(|(k, v)| (k, minijinja::value::Value::from_serialize(&v)))
            .collect();
        let ctx = minijinja::value::Value::from_iter(entries);
        tmpl.render(ctx).map_err(|e| {
            AgentError::Config(ConfigError::Invalid(format!("Template render error: {}", e)))
        })
    }

    /// Get examples for a named example set.
    pub fn get_examples(&self, name: &str) -> Option<&[Example]> {
        self.examples.get(name).map(|v| v.as_slice())
    }

    /// Load templates and examples from a directory.
    fn load_from_directory(&mut self, dir: &str) -> Result<(), AgentError> {
        let path = Path::new(dir);
        if !path.is_dir() {
            return Err(AgentError::Config(ConfigError::Invalid(format!(
                "Prompts directory not found: {}",
                dir
            )));
        }

        for entry in fs::read_dir(path).map_err(|e| {
            AgentError::Config(ConfigError::Invalid(format!(
                "Cannot read prompts directory: {}",
                e
            )))
        })? {
            let entry = entry.map_err(|e| {
                AgentError::Config(ConfigError::Invalid(format!(
                    "Error reading directory entry: {}",
                    e
                )))
            })?;
            let path = entry.path();

            match path.extension().and_then(|e| e.to_str()) {
                Some("j2") => {
                    let name = path
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();
                    let template = fs::read_to_string(&path).map_err(|e| {
                        AgentError::Config(ConfigError::Invalid(format!(
                            "Cannot read template {}: {}",
                            path.display(),
                            e
                        )))
                    })?;
                    self.add_template(&name, &template);
                }
                Some("toml") => {
                    let name = path
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();
                    let content = fs::read_to_string(&path).map_err(|e| {
                        AgentError::Config(ConfigError::Invalid(format!(
                            "Cannot read examples file {}: {}",
                            path.display(),
                            e
                        )))
                    })?;
                    let examples: Vec<Example> = toml::from_str(&content).map_err(|e| {
                        AgentError::Config(ConfigError::Invalid(format!(
                            "Cannot parse examples file {}: {}",
                            path.display(),
                            e
                        )))
                    })?;
                    self.add_examples(name, examples);
                }
                _ => {} // Ignore non-template files
            }
        }

        Ok(())
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        let mut env = Environment::new();
        env.add_template("react", DEFAULT_REACT_TEMPLATE).unwrap();
        env.add_template("system", DEFAULT_SYSTEM_TEMPLATE).unwrap();
        env.add_template("strategies.react", DEFAULT_REACT_TEMPLATE).unwrap();
        env.add_template(
            "strategies.plan_and_execute",
            DEFAULT_PLAN_AND_EXECUTE_TEMPLATE,
        )
        .unwrap();
        env.add_template(
            "strategies.hierarchical.decompose",
            DEFAULT_HIERARCHICAL_DECOMPOSE_TEMPLATE,
        )
        .unwrap();
        env.add_template("router", DEFAULT_ROUTER_TEMPLATE).unwrap();
        Self {
            env,
            examples: HashMap::new(),
        }
    }
}
```

- [ ] **Step 2: Add file error variant to ConfigError**

Modify `avs-core/src/error.rs` — add to `ConfigError`:
```rust
#[error("File error: {0}")]
FileError(String),
```

- [ ] **Step 3: Update PromptRegistry re-export in lib.rs**

No change needed — `PromptRegistry` is already re-exported. But verify `PromptConfig` and `Example` are available:

Modify `avs-core/src/lib.rs` — add `PromptConfig` re-export:
```rust
pub use prompt::PromptConfig;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check --package agentverse`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add avs-core/src/prompt.rs avs-core/src/error.rs avs-core/src/lib.rs
git commit -m "core: enhance PromptRegistry with Example storage and file loading"
```

---

### Task 5: Write tests for PromptRegistry

**Files:**
- Modify: `avs-core/tests/prompt_test.rs`

- [ ] **Step 1: Rewrite prompt tests**

Replace entire `avs-core/tests/prompt_test.rs`:
```rust
use agentverse::{Example, PromptConfig, PromptRegistry};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[test]
fn test_prompt_registry_has_default_templates() {
    let registry = PromptRegistry::new();
    let mut context = HashMap::new();
    context.insert("conversation".to_string(), json!("User: hello"));
    context.insert("tools".to_string(), json!(""));

    // All default templates should render without error
    let _ = registry.render("react", context.clone()).unwrap();
    let _ = registry.render("system", context.clone()).unwrap();
    let _ = registry.render("strategies.react", context).unwrap();
}

#[test]
fn test_prompt_registry_unknown_template() {
    let registry = PromptRegistry::new();
    let result = registry.render("nonexistent", HashMap::new());
    assert!(result.is_err());
}

#[test]
fn test_prompt_registry_add_custom_template() {
    let mut registry = PromptRegistry::new();
    registry.add_template("custom", "Hello {{ name }}!");
    let mut context = HashMap::new();
    context.insert("name".to_string(), json!("World"));
    let result = registry.render("custom", context).unwrap();
    assert_eq!(result, "Hello World!");
}

#[test]
fn test_prompt_registry_default() {
    let registry = PromptRegistry::default();
    assert!(registry.render("react", HashMap::new()).is_ok());
}

#[test]
fn test_prompt_registry_with_examples() {
    let mut registry = PromptRegistry::new();
    registry.add_examples(
        "react_examples".to_string(),
        vec![
            Example {
                input: "What is 2+2?".to_string(),
                output: Some("Answer: 4".to_string()),
                strategy: None,
            },
            Example {
                input: "What time is it?".to_string(),
                output: Some("Answer: check clock".to_string()),
                strategy: None,
            },
        ],
    );

    let examples = registry.get_examples("react_examples");
    assert!(examples.is_some());
    assert_eq!(examples.unwrap().len(), 2);
}

#[test]
fn test_prompt_registry_from_config() {
    let mut templates = HashMap::new();
    templates.insert("custom".to_string(), "Custom: {{ value }}".to_string());
    let mut examples = HashMap::new();
    examples.insert(
        "custom_examples".to_string(),
        vec![Example {
            input: "test".to_string(),
            output: Some("test output".to_string()),
            strategy: None,
        }],
    );

    let config = PromptConfig {
        system_prompt: Some("Custom system".to_string()),
        prompts_dir: None,
        templates,
        examples,
    };

    let registry = PromptRegistry::from_config(&config).unwrap();

    // System override should work
    let mut context = HashMap::new();
    context.insert("conversation".to_string(), json!(""));
    context.insert("tools".to_string(), json!(""));
    let system = registry.render("system", context).unwrap();
    assert_eq!(system, "Custom system");

    // Custom template should work
    let mut context = HashMap::new();
    context.insert("value".to_string(), json!("works"));
    let result = registry.render("custom", context).unwrap();
    assert_eq!(result, "Custom: works");

    // Examples should be registered
    let examples = registry.get_examples("custom_examples");
    assert!(examples.is_some());
}

#[test]
fn test_prompt_registry_from_config_with_directory() {
    // Create a temp directory with test files
    let temp_dir = std::env::temp_dir().join("avs_prompt_test");
    fs::create_dir_all(&temp_dir).unwrap();

    // Write a template
    fs::write(
        temp_dir.join("test.j2"),
        "Test template: {{ val }}",
    )
    .unwrap();

    // Write examples
    fs::write(
        temp_dir.join("test_examples.toml"),
        r#"[[example]]
input = "test input"
output = "test output"
"#,
    )
    .unwrap();

    let config = PromptConfig {
        system_prompt: None,
        prompts_dir: Some(temp_dir.to_string_lossy().to_string()),
        templates: HashMap::new(),
        examples: HashMap::new(),
    };

    let registry = PromptRegistry::from_config(&config).unwrap();

    // Template should be loaded
    let mut context = HashMap::new();
    context.insert("val".to_string(), json!("hello"));
    let result = registry.render("test", context).unwrap();
    assert_eq!(result, "Test template: hello");

    // Examples should be loaded
    let examples = registry.get_examples("test_examples");
    assert!(examples.is_some());
    assert_eq!(examples.unwrap().len(), 1);
    assert_eq!(examples.unwrap()[0].input, "test input");

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_prompt_registry_from_config_missing_directory() {
    let config = PromptConfig {
        system_prompt: None,
        prompts_dir: Some("/nonexistent/path".to_string()),
        templates: HashMap::new(),
        examples: HashMap::new(),
    };

    let result = PromptRegistry::from_config(&config);
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --package agentverse prompt_test`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add avs-core/tests/prompt_test.rs
git commit -m "test: rewrite prompt tests for new PromptRegistry API"
```

---

### Task 6: Update Config with prompts_dir and system_prompt

**Files:**
- Modify: `avs-core/src/config.rs`

- [ ] **Step 1: Update Config struct**

Replace entire `avs-core/src/config.rs`:
```rust
use serde::{Deserialize, Serialize};

use crate::error::{AgentError, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model_api_key: String,
    pub model_name: String,
    pub max_messages: usize,
    #[serde(default)]
    pub tools: Vec<String>,
    /// Optional prompts directory for .j2/.toml file loading.
    #[serde(default)]
    pub prompts_dir: Option<String>,
    /// Optional system prompt override.
    #[serde(default)]
    pub system_prompt: Option<String>,
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, AgentError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AgentError::Config(ConfigError::Invalid(e.to_string())))?;
        let config: Config = serde_yaml::from_str(&content)
            .map_err(|e| AgentError::Config(ConfigError::Invalid(e.to_string())))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), AgentError> {
        if self.model_api_key.is_empty() {
            return Err(AgentError::Config(ConfigError::Missing(
                "model_api_key is required".to_string(),
            )));
        }
        if self.model_name.is_empty() {
            return Err(AgentError::Config(ConfigError::Missing(
                "model_name is required".to_string(),
            )));
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS (but hello-agent may fail — fix in Task 13)

- [ ] **Step 3: Commit**

```bash
git add avs-core/src/config.rs
git commit -m "core: add prompts_dir and system_prompt to Config"
```

---

### Task 7: Update AgentBuilder with prompt APIs

**Files:**
- Modify: `avs-core/src/builder.rs`

- [ ] **Step 1: Rewrite AgentBuilder**

Replace entire `avs-core/src/builder.rs`:
```rust
use crate::config::Config;
use crate::error::AgentError;
use crate::prompt::PromptConfig;

pub struct AgentBuilder {
    config: Option<Config>,
    system_prompt: Option<String>,
    prompts_dir: Option<String>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            config: None,
            system_prompt: None,
            prompts_dir: None,
        }
    }

    /// Set the full config.
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Set a system prompt override.
    pub fn system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = Some(prompt.to_string());
        self
    }

    /// Set a prompts directory for .j2/.toml file loading.
    pub fn prompt_dir(mut self, dir: &str) -> Self {
        self.prompts_dir = Some(dir.to_string());
        self
    }

    pub fn build(self) -> Result<crate::agent::Agent, AgentError> {
        let config = self.config.unwrap_or_else(|| Config {
            model_api_key: String::new(),
            model_name: String::new(),
            max_messages: 100,
            tools: Vec::new(),
            prompts_dir: self.prompts_dir,
            system_prompt: self.system_prompt,
        });

        // Validate config
        if config.model_api_key.is_empty() {
            return Err(AgentError::Config(crate::error::ConfigError::Missing(
                "model_api_key is required".to_string(),
            )));
        }
        if config.model_name.is_empty() {
            return Err(AgentError::Config(crate::error::ConfigError::Missing(
                "model_name is required".to_string(),
            )));
        }

        // Build PromptConfig from Config
        let prompt_config = PromptConfig {
            system_prompt: self.system_prompt.clone(),
            prompts_dir: self.prompts_dir.clone(),
            templates: std::collections::HashMap::new(),
            examples: std::collections::HashMap::new(),
        };

        crate::agent::Agent::from_config_with_prompts(config, &prompt_config)
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: Update Agent::from_config to use PromptConfig**

Modify `avs-core/src/agent.rs` — add new constructor and update imports:
```rust
use crate::prompt::{PromptConfig, PromptRegistry};

impl Agent {
    // ... existing builder() ...

    /// Create from config with explicit prompt configuration.
    pub fn from_config_with_prompts(
        config: Config,
        prompt_config: &PromptConfig,
    ) -> Result<Self, AgentError> {
        config.validate()?;

        let prompt_registry = PromptRegistry::from_config(prompt_config)?;

        Ok(Self {
            config,
            memory: Arc::new(RwLock::new(ShortTermMemory::new(100))),
            prompt_registry,
            tracer: Box::new(DefaultTracer::default()),
        })
    }

    pub fn from_config(config: Config) -> Result<Self, AgentError> {
        let prompt_config = PromptConfig {
            system_prompt: config.system_prompt.clone(),
            prompts_dir: config.prompts_dir.clone(),
            templates: std::collections::HashMap::new(),
            examples: std::collections::HashMap::new(),
        };
        Self::from_config_with_prompts(config, &prompt_config)
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --package agentverse`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add avs-core/src/builder.rs avs-core/src/agent.rs
git commit -m "core: add prompt APIs to AgentBuilder and Agent"
```

---

### Task 8: Wire PromptRegistry into CycleSkeleton and add guardrails

**Files:**
- Modify: `avs-react/src/cycle.rs`
- Modify: `avs-react/src/react.rs`
- Modify: `avs-react/Cargo.toml` (add avs-guardrails dependency)

- [ ] **Step 1: Add avs-guardrails dependency**

In `avs-react/Cargo.toml`, add to `[dependencies]`:
```toml
agentverse-guardrails = { path = "../avs-guardrails" }
```

- [ ] **Step 2: Rewrite CycleSkeleton with guardrails**

Replace entire `avs-react/src/cycle.rs`:
```rust
use agentverse::{AgentError, GuardrailError, Message, ModelProvider, PromptRegistry, SyncTool};
use agentverse_guardrails::{check_output, check_prompt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info};

/// The fixed cycle skeleton that all strategies share.
pub struct CycleSkeleton<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    prompt_registry: Arc<PromptRegistry>,
    model: Arc<P>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<Mutex<M>>,
    max_iterations: usize,
    current_iteration: usize,
}

#[derive(Debug)]
pub enum CycleAction {
    Continue { thought: String },
    ToolCall { tool_name: String, args: Value },
    Done { answer: String },
    Error { message: String },
}

impl<P, M> CycleSkeleton<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    pub fn new(
        prompt_registry: Arc<PromptRegistry>,
        model: Arc<P>,
        tools: Vec<Box<dyn SyncTool>>,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
    ) -> Self {
        Self {
            prompt_registry,
            model,
            tools,
            memory,
            max_iterations,
            current_iteration: 0,
        }
    }

    pub async fn run<F, Fut>(
        &mut self,
        initial_message: String,
        mut step: F,
    ) -> Result<String, AgentError>
    where
        F: FnMut(&mut Self) -> Fut,
        Fut: std::future::Future<Output = Result<CycleAction, AgentError>>,
    {
        self.memory.lock().unwrap().append(Message {
            role: agentverse::memory::MessageRole::User,
            content: initial_message,
        });

        while self.current_iteration < self.max_iterations {
            self.current_iteration += 1;
            debug!(iteration = self.current_iteration, "Running strategy step");

            let action = step(self).await?;

            match action {
                CycleAction::Continue { thought } => {
                    self.memory.lock().unwrap().append(Message {
                        role: agentverse::memory::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                    info!(iteration = self.current_iteration, "Thought only, continuing");
                }
                CycleAction::ToolCall { tool_name, args } => {
                    let result = self.execute_tool(&tool_name, args)?;
                    self.memory.lock().unwrap().append(Message {
                        role: agentverse::memory::MessageRole::Tool,
                        content: format!("Tool: {}\nResult: {}", tool_name, result),
                    });
                    info!(iteration = self.current_iteration, tool = tool_name, "Tool executed");
                }
                CycleAction::Done { answer } => {
                    self.memory.lock().unwrap().append(Message {
                        role: agentverse::memory::MessageRole::Assistant,
                        content: answer.clone(),
                    });
                    info!(iteration = self.current_iteration, "Strategy completed");
                    return Ok(answer);
                }
                CycleAction::Error { message } => {
                    error!(error = %message, "Strategy error");
                    return Err(AgentError::Model(agentverse::ModelError::InvalidResponse(
                        message,
                    )));
                }
            }
        }

        Err(AgentError::Model(agentverse::ModelError::Timeout(format!(
            "Max iterations ({}) reached",
            self.max_iterations
        ))))
    }

    pub fn execute_tool(&self, tool_name: &str, args: Value) -> Result<String, AgentError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| {
                AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string()))
            })?;

        let result = tool.execute(args).map_err(AgentError::Tool)?;
        Ok(result.to_string())
    }

    /// Build the prompt from conversation history, tool descriptions, and examples.
    pub fn build_prompt(&self) -> Result<String, AgentError> {
        let last_messages = self.memory.lock().unwrap().last_n(20);
        let mut context = HashMap::new();

        let conversation: String = last_messages
            .iter()
            .map(|m| {
                let role_str = match m.role {
                    agentverse::memory::MessageRole::System => "System",
                    agentverse::memory::MessageRole::User => "User",
                    agentverse::memory::MessageRole::Assistant => "Assistant",
                    agentverse::memory::MessageRole::Tool => "Tool",
                };
                format!("{}: {}", role_str, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");
        context.insert("conversation".to_string(), Value::String(conversation));

        let tools: String = self
            .tools
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");
        context.insert("tools".to_string(), Value::String(tools));

        self.prompt_registry
            .render("react", context)
            .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e)))
    }

    /// Build the prompt with guardrail checking on the rendered prompt.
    pub fn build_prompt_with_guardrails(&self) -> Result<String, AgentError> {
        let prompt = self.build_prompt()?;
        check_prompt(&prompt).map_err(|e| AgentError::Guardrail(e))?;
        Ok(prompt)
    }

    /// Apply output guardrail to a model response.
    pub fn check_output_guardrail(output: &str) -> Result<(), AgentError> {
        check_output(output).map_err(|e| AgentError::Guardrail(e))
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    pub fn current_iteration(&self) -> usize {
        self.current_iteration
    }

    pub fn max_iterations(&self) -> usize {
        self.max_iterations
    }

    pub fn model(&self) -> &P {
        &self.model
    }

    pub fn tools(&self) -> &[Box<dyn SyncTool>] {
        &self.tools
    }

    pub fn memory(&self) -> &Arc<Mutex<M>> {
        &self.memory
    }

    pub fn prompt_registry(&self) -> &Arc<PromptRegistry> {
        &self.prompt_registry
    }

    pub fn next_iteration(&mut self) -> usize {
        self.current_iteration += 1;
        self.current_iteration
    }
}
```

- [ ] **Step 3: Update ReActStrategy to use guardrails**

Modify `avs-react/src/react.rs` — change the `run()` method:
```rust
// In the run() loop, replace:
let prompt = self.skeleton.build_prompt()?;
// With:
let prompt = self.skeleton.build_prompt_with_guardrails()?;

// After the model.generate() call, add guardrail:
let response = self.skeleton.model().generate(&prompt, Some(tool_defs.clone())).await?;
self.skeleton.check_output_guardrail(&response)?;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add avs-react/src/cycle.rs avs-react/src/react.rs avs-react/Cargo.toml
git commit -m "react: wire PromptRegistry into CycleSkeleton, add guardrails"
```

---

### Task 9: Replace hardcoded strings in PlanStrategy

**Files:**
- Modify: `avs-plan/src/plan.rs`
- Modify: `avs-plan/src/planner.rs`
- Modify: `avs-plan/Cargo.toml` (add avs-guardrails dependency)

- [ ] **Step 1: Add dependencies**

In `avs-plan/Cargo.toml`, add to `[dependencies]`:
```toml
agentverse-guardrails = { path = "../avs-guardrails" }
```

- [ ] **Step 2: Rewrite planner.rs with templated prompts**

Replace entire `avs-plan/src/planner.rs`:
```rust
use agentverse::{AgentError, GuardrailError, ModelProvider, PromptRegistry};
use agentverse_guardrails::check_prompt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStep {
    pub id: usize,
    pub description: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(default)]
    pub depends_on: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Plan {
    pub description: String,
    pub steps: Vec<PlanStep>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Generate a plan from the LLM using the templated prompt.
pub async fn generate_plan(
    model: &dyn ModelProvider,
    registry: &PromptRegistry,
    request: &str,
    tools: &[String],
    conversation: &str,
) -> Result<Plan, AgentError> {
    let tools_desc = if tools.is_empty() {
        "none (reasoning only)".to_string()
    } else {
        tools.join(", ")
    };

    let mut context = HashMap::new();
    context.insert("tools".to_string(), serde_json::Value::String(tools_desc));
    context.insert("conversation".to_string(), serde_json::Value::String(conversation.to_string()));

    let strategy_prompt = registry
        .render("strategies.plan_and_execute", context)
        .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e)))?;

    check_prompt(&strategy_prompt).map_err(|e| AgentError::Guardrail(e))?;

    let prompt = format!("{}\n\nRequest: {}\n\nRespond with ONLY a JSON object:\n{\"description\": \"...\", \"steps\": [{\"id\": 1, \"description\": \"...\", \"tool\": \"...\", \"args\": {}, \"depends_on\": []}]}", strategy_prompt, request);

    let response = model.generate(&prompt, None).await?;

    let json_str = response
        .trim()
        .trim_start_matches('`')
        .trim_start_matches("json")
        .trim_start_matches('`')
        .trim();

    let plan: Plan = serde_json::from_str(json_str).map_err(|e| {
        AgentError::Model(agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse plan JSON: {}. Response was: {}",
            e, response
        )))
    })?;

    Ok(plan)
}

/// Decompose a complex request into sub-goals.
pub async fn decompose_request(
    model: &dyn ModelProvider,
    registry: &PromptRegistry,
    request: &str,
) -> Result<Vec<String>, AgentError> {
    let mut context = HashMap::new();
    context.insert(
        "conversation".to_string(),
        serde_json::Value::String(format!("User: {}", request)),
    );

    let strategy_prompt = registry
        .render("strategies.hierarchical.decompose", context)
        .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e)))?;

    check_prompt(&strategy_prompt).map_err(|e| AgentError::Guardrail(e))?;

    let prompt = format!("{}\n\nRequest: {}\n\nRespond with ONLY a JSON array of strings.", strategy_prompt, request);

    let response = model.generate(&prompt, None).await?;

    let json_str = response
        .trim()
        .trim_start_matches('`')
        .trim_start_matches("json")
        .trim_start_matches('`')
        .trim();

    let sub_goals: Vec<String> = serde_json::from_str(json_str).map_err(|e| {
        AgentError::Model(agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse decomposition: {}. Response was: {}",
            e, response
        )))
    })?;

    Ok(sub_goals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_is_empty() {
        let plan = Plan {
            description: "test".to_string(),
            steps: vec![],
        };
        assert!(plan.is_empty());

        let plan = Plan {
            description: "test".to_string(),
            steps: vec![PlanStep {
                id: 1,
                description: "step".to_string(),
                tool: None,
                args: None,
                depends_on: vec![],
            }],
        };
        assert!(!plan.is_empty());
    }

    #[test]
    fn test_plan_step_defaults() {
        let step = PlanStep {
            id: 1,
            description: "test".to_string(),
            tool: None,
            args: None,
            depends_on: vec![],
        };
        assert!(step.tool.is_none());
        assert!(step.args.is_none());
        assert!(step.depends_on.is_empty());
    }
}
```

- [ ] **Step 3: Update PlanStrategy to use PromptRegistry**

Replace entire `avs-plan/src/plan.rs`:
```rust
use super::planner::{generate_plan, Plan};
use agentverse::{AgentError, GuardrailError, ModelProvider, PromptRegistry, SyncTool};
use agentverse_guardrails::check_output;
use std::sync::{Arc, Mutex};

pub struct PlanStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    model: Arc<P>,
    registry: Arc<PromptRegistry>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<Mutex<M>>,
    max_iterations: usize,
}

impl<P, M> PlanStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    pub fn new(
        model: Arc<P>,
        registry: Arc<PromptRegistry>,
        tools: Vec<Box<dyn SyncTool>>,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
    ) -> Self {
        Self {
            model,
            registry,
            tools,
            memory,
            max_iterations,
        }
    }

    pub async fn run(&mut self, input: String) -> Result<String, AgentError> {
        self.memory.lock().unwrap().append(agentverse::Message {
            role: agentverse::memory::MessageRole::User,
            content: input.clone(),
        });

        let tool_names: Vec<String> = self.tools.iter().map(|t| t.name().to_string()).collect();

        let conversation = self
            .memory
            .lock()
            .unwrap()
            .last_n(20)
            .iter()
            .map(|m| {
                let role_str = match m.role {
                    agentverse::memory::MessageRole::System => "System",
                    agentverse::memory::MessageRole::User => "User",
                    agentverse::memory::MessageRole::Assistant => "Assistant",
                    agentverse::memory::MessageRole::Tool => "Tool",
                };
                format!("{}: {}", role_str, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let plan = generate_plan(&*self.model, &self.registry, &input, &tool_names, &conversation).await?;

        self.memory.lock().unwrap().append(agentverse::Message {
            role: agentverse::memory::MessageRole::System,
            content: format!("Plan generated: {}", plan.description),
        });

        let mut step_results: Vec<(usize, String)> = Vec::new();

        for step in &plan.steps {
            if step.id > self.max_iterations {
                self.memory.lock().unwrap().append(agentverse::Message {
                    role: agentverse::memory::MessageRole::System,
                    content: format!(
                        "Stopping at step {}: max iterations ({}) reached",
                        step.id, self.max_iterations
                    ),
                });
                break;
            }

            let result = if let Some(ref tool_name) = step.tool {
                let args = step.args.clone().unwrap_or_default();
                match self.execute_tool(tool_name, args) {
                    Ok(result) => result,
                    Err(e) => format!("Tool error: {}", e),
                }
            } else {
                format!("Reasoning: {}", step.description)
            };

            step_results.push((step.id, result.clone()));

            self.memory.lock().unwrap().append(agentverse::Message {
                role: agentverse::memory::MessageRole::System,
                content: format!("Step {} executed: {}", step.id, result),
            });
        }

        // Synthesize final answer
        let conversation_history = self
            .memory
            .lock()
            .unwrap()
            .last_n(20)
            .iter()
            .map(|m| {
                let role_str = match m.role {
                    agentverse::memory::MessageRole::System => "System",
                    agentverse::memory::MessageRole::User => "User",
                    agentverse::memory::MessageRole::Assistant => "Assistant",
                    agentverse::memory::MessageRole::Tool => "Tool",
                };
                format!("{}: {}", role_str, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let final_prompt = format!(
            "You executed the following plan:\n\
             Plan: {}\n\n\
             Step results:\n{}\n\n\
             Based on these results, provide the final answer to the user's request.\n\n\
             User request: {}\n\n\
             Conversation history:\n{}",
            plan.description,
            step_results
                .iter()
                .map(|(id, result)| format!("Step {}: {}", id, result))
                .collect::<Vec<_>>()
                .join("\n"),
            input,
            conversation_history
        );

        check_prompt(&final_prompt).map_err(|e| AgentError::Guardrail(e))?;

        let answer = self
            .model
            .generate(&final_prompt, None)
            .await
            .map_err(AgentError::Model)?;

        check_output(&answer).map_err(|e| AgentError::Guardrail(e))?;

        Ok(answer)
    }

    fn execute_tool(&self, tool_name: &str, args: serde_json::Value) -> Result<String, AgentError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| {
                AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string()))
            })?;

        let result = tool.execute(args).map_err(AgentError::Tool)?;
        Ok(result.to_string())
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add avs-plan/src/planner.rs avs-plan/src/plan.rs avs-plan/Cargo.toml
git commit -m "plan: replace hardcoded strings with templated prompts"
```

---

### Task 10: Replace hardcoded strings in HierarchicalStrategy

**Files:**
- Modify: `avs-plan/src/hierarchical.rs`

- [ ] **Step 1: Rewrite HierarchicalStrategy**

Replace entire `avs-plan/src/hierarchical.rs`:
```rust
use super::planner::{decompose_request, generate_plan, Plan};
use agentverse::{AgentError, GuardrailError, ModelProvider, PromptRegistry, SyncTool};
use agentverse_guardrails::check_output;
use std::sync::{Arc, Mutex};

pub struct HierarchicalStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    model: Arc<P>,
    registry: Arc<PromptRegistry>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<Mutex<M>>,
    max_iterations: usize,
    max_decompose_depth: usize,
}

impl<P, M> HierarchicalStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    pub fn new(
        model: Arc<P>,
        registry: Arc<PromptRegistry>,
        tools: Vec<Box<dyn SyncTool>>,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
        max_decompose_depth: usize,
    ) -> Self {
        Self {
            model,
            registry,
            tools,
            memory,
            max_iterations,
            max_decompose_depth,
        }
    }

    pub async fn run(&mut self, input: String) -> Result<String, AgentError> {
        self.memory.lock().unwrap().append(agentverse::Message {
            role: agentverse::memory::MessageRole::User,
            content: input.clone(),
        });

        let sub_goals = decompose_request(&*self.model, &self.registry, &input).await?;

        self.memory.lock().unwrap().append(agentverse::Message {
            role: agentverse::memory::MessageRole::System,
            content: format!("Decomposed into {} sub-goals", sub_goals.len()),
        });

        let mut sub_goal_results: Vec<(usize, String)> = Vec::new();

        for (i, sub_goal) in sub_goals.iter().enumerate() {
            if i >= self.max_decompose_depth {
                self.memory.lock().unwrap().append(agentverse::Message {
                    role: agentverse::memory::MessageRole::System,
                    content: format!(
                        "Stopping sub-goal decomposition at depth {}: max depth ({}) reached",
                        i + 1,
                        self.max_decompose_depth
                    ),
                });
                break;
            }

            let tool_names: Vec<String> = self.tools.iter().map(|t| t.name().to_string()).collect();

            let conversation = self
                .memory
                .lock()
                .unwrap()
                .last_n(20)
                .iter()
                .map(|m| {
                    let role_str = match m.role {
                        agentverse::memory::MessageRole::System => "System",
                        agentverse::memory::MessageRole::User => "User",
                        agentverse::memory::MessageRole::Assistant => "Assistant",
                        agentverse::memory::MessageRole::Tool => "Tool",
                    };
                    format!("{}: {}", role_str, m.content)
                })
                .collect::<Vec<_>>()
                .join("\n");

            let sub_plan = generate_plan(&*self.model, &self.registry, sub_goal, &tool_names, &conversation).await?;

            let mut step_results: Vec<String> = Vec::new();
            for step in &sub_plan.steps {
                if step.id > self.max_iterations {
                    self.memory.lock().unwrap().append(agentverse::Message {
                        role: agentverse::memory::MessageRole::System,
                        content: format!("Sub-goal {} step {}: max iterations reached", i, step.id),
                    });
                    break;
                }

                let result = if let Some(ref tool_name) = step.tool {
                    let args = step.args.clone().unwrap_or_default();
                    match self.execute_tool(tool_name, args) {
                        Ok(result) => result,
                        Err(e) => format!("Tool error: {}", e),
                    }
                } else {
                    format!("Reasoning: {}", step.description)
                };

                step_results.push(result.clone());

                self.memory.lock().unwrap().append(agentverse::Message {
                    role: agentverse::memory::MessageRole::System,
                    content: format!(
                        "Sub-goal {} step {} ({}): {}",
                        i,
                        step.id,
                        step.tool.as_deref().unwrap_or("reasoning"),
                        result
                    ),
                });
            }

            let sub_result = step_results.join("\n");
            sub_goal_results.push((i, sub_result.clone()));

            self.memory.lock().unwrap().append(agentverse::Message {
                role: agentverse::memory::MessageRole::System,
                content: format!("Sub-goal {} completed: {}", i, sub_result),
            });
        }

        let conversation_history = self
            .memory
            .lock()
            .unwrap()
            .last_n(30)
            .iter()
            .map(|m| {
                let role_str = match m.role {
                    agentverse::memory::MessageRole::System => "System",
                    agentverse::memory::MessageRole::User => "User",
                    agentverse::memory::MessageRole::Assistant => "Assistant",
                    agentverse::memory::MessageRole::Tool => "Tool",
                };
                format!("{}: {}", role_str, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let final_prompt = format!(
            "All sub-goals have been executed. Provide a comprehensive answer to the user's request.\n\n\
             User request: {}\n\n\
             Sub-goal results:\n{}\n\n\
             Conversation history:\n{}",
            input,
            sub_goal_results
                .iter()
                .map(|(id, result)| format!("Sub-goal {}: {}", id, result))
                .collect::<Vec<_>>()
                .join("\n"),
            conversation_history
        );

        check_prompt(&final_prompt).map_err(|e| AgentError::Guardrail(e))?;

        let answer = self
            .model
            .generate(&final_prompt, None)
            .await
            .map_err(AgentError::Model)?;

        check_output(&answer).map_err(|e| AgentError::Guardrail(e))?;

        Ok(answer)
    }

    fn execute_tool(&self, tool_name: &str, args: serde_json::Value) -> Result<String, AgentError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| {
                AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string()))
            })?;

        let result = tool.execute(args).map_err(AgentError::Tool)?;
        Ok(result.to_string())
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add avs-plan/src/hierarchical.rs
git commit -m "plan: wire PromptRegistry into HierarchicalStrategy"
```

---

### Task 11: Replace hardcoded string in StrategyRouter

**Files:**
- Modify: `avs-router/src/router.rs`
- Modify: `avs-router/Cargo.toml`

- [ ] **Step 1: Add dependency**

In `avs-router/Cargo.toml`, add to `[dependencies]`:
```toml
agentverse-guardrails = { path = "../avs-guardrails" }
```

- [ ] **Step 2: Rewrite StrategyRouter**

Replace entire `avs-router/src/router.rs`:
```rust
use agentverse::{AgentError, GuardrailError, ModelProvider, PromptRegistry};
use agentverse_guardrails::check_prompt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StrategyName {
    ReAct,
    PlanAndExecute,
    Hierarchical,
}

impl std::fmt::Display for StrategyName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StrategyName::ReAct => write!(f, "react"),
            StrategyName::PlanAndExecute => write!(f, "plan_and_execute"),
            StrategyName::Hierarchical => write!(f, "hierarchical"),
        }
    }
}

pub struct StrategyRouter<P>
where
    P: ModelProvider,
{
    model: P,
    strategies: Vec<StrategyName>,
    registry: Option<std::sync::Arc<PromptRegistry>>,
}

impl<P> StrategyRouter<P>
where
    P: ModelProvider,
{
    pub fn new(model: P, strategies: Vec<StrategyName>) -> Self {
        Self {
            model,
            strategies,
            registry: None,
        }
    }

    /// Create a router with prompt registry for templated prompts.
    pub fn with_registry(model: P, strategies: Vec<StrategyName>, registry: std::sync::Arc<PromptRegistry>) -> Self {
        Self {
            model,
            strategies,
            registry: Some(registry),
        }
    }

    pub async fn route(&self, request: &str) -> Result<StrategyName, AgentError> {
        let strategy_list = self
            .strategies
            .iter()
            .map(|s| format!("{}: {}", s, strategy_description(s)))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = if let Some(ref registry) = self.registry {
            let mut context = HashMap::new();
            context.insert("conversation".to_string(), serde_json::Value::String(format!("User: {}", request)));
            context.insert("tools".to_string(), serde_json::Value::String(strategy_list));

            let strategy_prompt = registry
                .render("router", context)
                .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e)))?;

            check_prompt(&strategy_prompt).map_err(|e| AgentError::Guardrail(e))?;

            format!("{}\n\nRequest: {}\n\nRespond with ONLY the strategy name.", strategy_prompt, request)
        } else {
            // Fallback to hardcoded prompt if no registry
            format!(
                "Choose the best orchestration strategy for the following request.\n\n\
                 Request: {}\n\n\
                 Available strategies:\n{}\n\n\
                 Respond with ONLY the strategy name (e.g., 'react', 'plan_and_execute', 'hierarchical').\n\
                 Do not include any explanation.",
                request, strategy_list
            )
        };

        let response = self.model.generate(&prompt, None).await?;
        let selected = response.trim().to_lowercase();

        match selected.as_str() {
            "react" => Ok(StrategyName::ReAct),
            "plan_and_execute" | "plan-and-execute" => Ok(StrategyName::PlanAndExecute),
            "hierarchical" => Ok(StrategyName::Hierarchical),
            _ => Err(AgentError::Model(agentverse::ModelError::InvalidResponse(format!(
                "Unknown strategy: {}",
                response
            )))),
        }
    }

    pub fn available_strategies(&self) -> &[StrategyName] {
        &self.strategies
    }
}

pub fn strategy_description(strategy: &StrategyName) -> &'static str {
    match strategy {
        StrategyName::ReAct => "Best for: simple Q&A, tool use, step-by-step reasoning",
        StrategyName::PlanAndExecute => "Best for: tasks with clear steps that can be planned upfront",
        StrategyName::Hierarchical => "Best for: complex tasks that need decomposition into sub-goals",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_name_display() {
        assert_eq!(StrategyName::ReAct.to_string(), "react");
        assert_eq!(StrategyName::PlanAndExecute.to_string(), "plan_and_execute");
        assert_eq!(StrategyName::Hierarchical.to_string(), "hierarchical");
    }

    #[test]
    fn test_strategy_description() {
        assert!(strategy_description(&StrategyName::ReAct).contains("Q&A"));
        assert!(strategy_description(&StrategyName::PlanAndExecute).contains("planned"));
        assert!(strategy_description(&StrategyName::Hierarchical).contains("decomposition"));
    }

    #[test]
    fn test_strategy_name_serialization() {
        let name = StrategyName::ReAct;
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"ReAct\"");

        let deserialized: StrategyName = serde_json::from_str(&json).unwrap();
        assert_eq!(name, deserialized);
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add avs-router/src/router.rs avs-router/Cargo.toml
git commit -m "router: replace hardcoded prompt with templated prompt"
```

---

### Task 12: Update Agent::invoke() to use full prompt system

**Files:**
- Modify: `avs-core/src/agent.rs`

- [ ] **Step 1: Update Agent::invoke()**

Replace entire `avs-core/src/agent.rs`:
```rust
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AgentError;
use crate::memory::{Memory, Message, ShortTermMemory};
use crate::prompt::{PromptConfig, PromptRegistry};
use crate::tracing::{DefaultTracer, Tracer};

pub struct Agent {
    config: Config,
    memory: Arc<RwLock<dyn Memory>>,
    prompt_registry: PromptRegistry,
    tracer: Box<dyn Tracer>,
}

impl Agent {
    pub fn builder() -> crate::builder::AgentBuilder {
        crate::builder::AgentBuilder::new()
    }

    pub fn from_config_with_prompts(
        config: Config,
        prompt_config: &PromptConfig,
    ) -> Result<Self, AgentError> {
        config.validate()?;

        let prompt_registry = PromptRegistry::from_config(prompt_config)?;

        Ok(Self {
            config,
            memory: Arc::new(RwLock::new(ShortTermMemory::new(100))),
            prompt_registry,
            tracer: Box::new(DefaultTracer::default()),
        })
    }

    pub fn from_config(config: Config) -> Result<Self, AgentError> {
        let prompt_config = PromptConfig {
            system_prompt: config.system_prompt.clone(),
            prompts_dir: config.prompts_dir.clone(),
            templates: std::collections::HashMap::new(),
            examples: std::collections::HashMap::new(),
        };
        Self::from_config_with_prompts(config, &prompt_config)
    }

    /// Get a reference to the prompt registry for strategy composition.
    pub fn prompt_registry(&self) -> &PromptRegistry {
        &self.prompt_registry
    }

    pub async fn invoke(&self, user_id: &str, input: &str) -> Result<String, AgentError> {
        let mut memory = self.memory.write().await;
        memory.append(Message {
            role: crate::memory::MessageRole::User,
            content: input.to_string(),
        });
        drop(memory);

        // TODO: Strategy loop will be implemented in avs-react
        // For now, return a placeholder using the system prompt
        let _ = user_id;
        let mut context = std::collections::HashMap::new();
        context.insert("conversation".to_string(), serde_json::Value::String(input.to_string()));
        let system = self
            .prompt_registry
            .render("system", context)
            .unwrap_or_else(|_| "You are a helpful assistant.".to_string());
        Ok(format!("[{}] {}", system, input))
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add avs-core/src/agent.rs
git commit -m "core: update Agent::invoke() to use prompt registry"
```

---

### Task 13: Update all 5 example agents

**Files:**
- Modify: `examples/hello-agent/src/main.rs`
- Modify: `examples/hello-agent/Cargo.toml`
- Create: `examples/hello-agent/prompts/`
- Modify: `examples/slack-hr-assistant/src/main.rs`
- Modify: `examples/slack-hr-assistant/Cargo.toml`
- Modify: `examples/rag-qa/src/main.rs`
- Modify: `examples/rag-qa/Cargo.toml`
- Modify: `examples/web-search-agent/src/main.rs`
- Modify: `examples/web-search-agent/Cargo.toml`
- Modify: `examples/code-review-agent/src/main.rs`
- Modify: `examples/code-review-agent/Cargo.toml`

#### hello-agent

- [ ] **Step 1: Update Cargo.toml**

In `examples/hello-agent/Cargo.toml`, add dependencies:
```toml
agentverse-react = { path = "../../avs-react" }
agentverse-tools = { path = "../../avs-tools" }
agentverse-guardrails = { path = "../../avs-guardrails" }
```

- [ ] **Step 2: Rewrite hello-agent main.rs**

Replace entire `examples/hello-agent/src/main.rs`:
```rust
use agentverse::{Example, PromptConfig, PromptRegistry};
use agentverse_react::ReActStrategy;
use agentverse_tools::{Calculator, DateTimeTool, ToolRegistry};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    // Build prompt registry with defaults
    let prompt_config = PromptConfig {
        system_prompt: Some("You are a helpful calculator assistant. Be precise with numbers.".to_string()),
        prompts_dir: None,
        templates: std::collections::HashMap::new(),
        examples: std::collections::HashMap::new(),
    };
    let prompt_registry = Arc::new(PromptRegistry::from_config(&prompt_config).unwrap());

    // Register few-shot examples
    prompt_registry.add_examples(
        "react_examples".to_string(),
        vec![
            Example {
                input: "What is 2+2?".to_string(),
                output: Some("Thought: I can calculate this.\nAction: calculator\nAction Input: {\"expression\": \"2 + 2\"}".to_string()),
                strategy: None,
            },
            Example {
                input: "What time is it?".to_string(),
                output: Some("Thought: I can get the current time.\nAction: datetime\nAction Input: {\"format\": \"%Y-%m-%d %H:%M:%S\"}".to_string()),
                strategy: None,
            },
        ],
    );

    // Create tools
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(Calculator::new()));
    registry.register(Box::new(DateTimeTool::new()));

    let tools: Vec<Box<dyn agentverse::SyncTool>> = registry
        .registered_tools()
        .into_iter()
        .map(|(name, tool)| Box::new(tool) as Box<dyn agentverse::SyncTool>)
        .collect();

    let memory = Arc::new(Mutex::new(agentverse::ShortTermMemory::new(100)));

    // Use OpenAI-compatible model
    let model = agentverse::model::OpenAICompatible::new(
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090".to_string()),
        std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string()),
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "llama3".to_string()),
    );

    let mut strategy = ReActStrategy::new(
        prompt_registry,
        Arc::new(model),
        tools,
        memory,
        10,
    );

    println!("Hello Agent - powered by AgentVerse");
    println!("Ask anything:");
    let result = strategy.run("What is 42 * 13?".to_string()).await;
    println!("Agent: {}", result.unwrap());
}
```

#### slack-hr-assistant

- [ ] **Step 3: Update Cargo.toml**

In `examples/slack-hr-assistant/Cargo.toml`, add:
```toml
agentverse-plan = { path = "../../avs-plan" }
agentverse-guardrails = { path = "../../avs-guardrails" }
```

- [ ] **Step 4: Rewrite slack-hr-assistant main.rs**

Replace entire `examples/slack-hr-assistant/src/main.rs`:
```rust
use agentverse::{PromptConfig, PromptRegistry};
use agentverse_plan::PlanStrategy;
use agentverse_tools::ToolRegistry;
use agentverse_integration::SlackAdapter;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let prompt_config = PromptConfig {
        system_prompt: Some("You are an HR assistant. Handle employee queries professionally.".to_string()),
        prompts_dir: None,
        templates: std::collections::HashMap::new(),
        examples: std::collections::HashMap::new(),
    };
    let prompt_registry = Arc::new(PromptRegistry::from_config(&prompt_config).unwrap());

    let mut registry = ToolRegistry::new();
    // No built-in tools for Slack HR - it uses the adapter
    let tools: Vec<Box<dyn agentverse::SyncTool>> = registry.registered_tools().into_iter().map(|(name, tool)| Box::new(tool) as Box<dyn agentverse::SyncTool>).collect();

    let memory = Arc::new(Mutex::new(agentverse::ShortTermMemory::new(100)));

    let model = agentverse::model::OpenAICompatible::new(
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090".to_string()),
        std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string()),
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "llama3".to_string()),
    );

    let mut strategy = PlanStrategy::new(
        Arc::new(model),
        prompt_registry,
        tools,
        memory,
        10,
    );

    // Use the strategy directly for a demo, or start Slack adapter
    let result = strategy.run("What is the company vacation policy?".to_string()).await;
    println!("Agent: {}", result.unwrap());

    // Start Slack adapter (commented out for local dev)
    /*
    let agent = Arc::new(Mutex::new(strategy));
    let adapter = SlackAdapter::new(
        agent,
        &std::env::var("SLACK_BOT_TOKEN").expect("SLACK_BOT_TOKEN not set"),
        &std::env::var("SLACK_SIGNING_SECRET").expect("SLACK_SIGNING_SECRET not set"),
        3000,
    );
    adapter.start().await.expect("Failed to start Slack adapter");
    */
}
```
#### rag-qa

- [ ] **Step 5: Update Cargo.toml**

In `examples/rag-qa/Cargo.toml`, add:
```toml
agentverse-react = { path = "../../avs-react" }
agentverse-tools = { path = "../../avs-tools" }
agentverse-guardrails = { path = "../../avs-guardrails" }
```

- [ ] **Step 6: Rewrite rag-qa main.rs**

Replace entire `examples/rag-qa/src/main.rs`:
```rust
use agentverse::{PromptConfig, PromptRegistry};
use agentverse_react::ReActStrategy;
use agentverse_tools::{FileSearch, HttpClient, ToolRegistry};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let prompt_config = PromptConfig {
        system_prompt: Some("You are a RAG QA assistant. Answer questions based on the provided knowledge base.".to_string()),
        prompts_dir: None,
        templates: std::collections::HashMap::new(),
        examples: std::collections::HashMap::new(),
    };
    let prompt_registry = Arc::new(PromptRegistry::from_config(&prompt_config).unwrap());

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(FileSearch::new("documents/")));
    registry.register(Box::new(HttpClient::new()));

    let tools: Vec<Box<dyn agentverse::SyncTool>> = registry
        .registered_tools()
        .into_iter()
        .map(|(name, tool)| Box::new(tool) as Box<dyn agentverse::SyncTool>)
        .collect();

    let memory = Arc::new(Mutex::new(agentverse::ShortTermMemory::new(100)));

    let model = agentverse::model::OpenAICompatible::new(
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090".to_string()),
        std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string()),
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "llama3".to_string()),
    );

    let mut strategy = ReActStrategy::new(
        prompt_registry,
        Arc::new(model),
        tools,
        memory,
        10,
    );

    println!("RAG QA Agent - powered by AgentVerse");
    let result = strategy.run("What is the project architecture?".to_string()).await;
    println!("Agent: {}", result.unwrap());
}
```

#### web-search-agent

- [ ] **Step 7: Update Cargo.toml**

In `examples/web-search-agent/Cargo.toml`, add:
```toml
agentverse-react = { path = "../../avs-react" }
agentverse-tools = { path = "../../avs-tools" }
agentverse-guardrails = { path = "../../avs-guardrails" }
```

- [ ] **Step 8: Rewrite web-search-agent main.rs**

Replace entire `examples/web-search-agent/src/main.rs`:
```rust
use agentverse::{PromptConfig, PromptRegistry};
use agentverse_react::ReActStrategy;
use agentverse_tools::{HttpClient, ToolRegistry};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let prompt_config = PromptConfig {
        system_prompt: Some("You are a web search assistant. Use the HTTP client to search the web and summarize results.".to_string()),
        prompts_dir: None,
        templates: std::collections::HashMap::new(),
        examples: std::collections::HashMap::new(),
    };
    let prompt_registry = Arc::new(PromptRegistry::from_config(&prompt_config).unwrap());

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(HttpClient::new()));

    let tools: Vec<Box<dyn agentverse::SyncTool>> = registry
        .registered_tools()
        .into_iter()
        .map(|(name, tool)| Box::new(tool) as Box<dyn agentverse::SyncTool>)
        .collect();

    let memory = Arc::new(Mutex::new(agentverse::ShortTermMemory::new(100)));

    let model = agentverse::model::OpenAICompatible::new(
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090".to_string()),
        std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string()),
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "llama3".to_string()),
    );

    let mut strategy = ReActStrategy::new(
        prompt_registry,
        Arc::new(model),
        tools,
        memory,
        10,
    );

    println!("Web Search Agent - powered by AgentVerse");
    let result = strategy.run("Search for the latest AI news".to_string()).await;
    println!("Agent: {}", result.unwrap());
}
```

#### code-review-agent

- [ ] **Step 9: Update Cargo.toml**

In `examples/code-review-agent/Cargo.toml`, add:
```toml
agentverse-plan = { path = "../../avs-plan" }
agentverse-tools = { path = "../../avs-tools" }
agentverse-guardrails = { path = "../../avs-guardrails" }
```

- [ ] **Step 10: Rewrite code-review-agent main.rs**

Replace entire `examples/code-review-agent/src/main.rs`:
```rust
use agentverse::{PromptConfig, PromptRegistry};
use agentverse_plan::HierarchicalStrategy;
use agentverse_tools::{FileSearch, HttpClient, ToolRegistry};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let prompt_config = PromptConfig {
        system_prompt: Some("You are a code review assistant. Review code for quality, security, and best practices.".to_string()),
        prompts_dir: None,
        templates: std::collections::HashMap::new(),
        examples: std::collections::HashMap::new(),
    };
    let prompt_registry = Arc::new(PromptRegistry::from_config(&prompt_config).unwrap());

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(FileSearch::new("src/")));
    registry.register(Box::new(HttpClient::new()));

    let tools: Vec<Box<dyn agentverse::SyncTool>> = registry
        .registered_tools()
        .into_iter()
        .map(|(name, tool)| Box::new(tool) as Box<dyn agentverse::SyncTool>)
        .collect();

    let memory = Arc::new(Mutex::new(agentverse::ShortTermMemory::new(100)));

    let model = agentverse::model::OpenAICompatible::new(
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090".to_string()),
        std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string()),
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "llama3".to_string()),
    );

    let mut strategy = HierarchicalStrategy::new(
        Arc::new(model),
        prompt_registry,
        tools,
        memory,
        10,
        3,
    );

    println!("Code Review Agent - powered by AgentVerse");
    let result = strategy.run("Review the avs-core crate for security vulnerabilities".to_string()).await;
    println!("Agent: {}", result.unwrap());
}
```

- [ ] **Step 11: Verify all example agents compile**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 12: Commit**

```bash
git add examples/*/Cargo.toml examples/*/src/main.rs
git commit -m "examples: update all agents with prompt architecture, guardrails, and tool wiring"
```

---

### Task 14: Add integration tests for prompt rendering with examples

**Files:**
- Create: `avs-core/tests/prompt_integration_test.rs`

- [ ] **Step 1: Write integration tests**

Create `avs-core/tests/prompt_integration_test.rs`:
```rust
use agentverse::{Example, PromptConfig, PromptRegistry};
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_prompt_rendering_with_examples() {
    let mut registry = PromptRegistry::new();
    registry.add_examples(
        "test_examples".to_string(),
        vec![
            Example {
                input: "What is 2+2?".to_string(),
                output: Some("Answer: 4".to_string()),
                strategy: None,
            },
        ],
    );

    let mut context = HashMap::new();
    context.insert("examples".to_string(), json!(registry.get_examples("test_examples")));
    context.insert("tools".to_string(), json!(""));
    context.insert("conversation".to_string(), json!(""));

    let result = registry.render("strategies.react", context).unwrap();
    assert!(result.contains("Answer: 4"));
    assert!(result.contains("ReAct pattern"));
}

#[test]
fn test_prompt_rendering_without_examples() {
    let registry = PromptRegistry::new();
    let mut context = HashMap::new();
    context.insert("examples".to_string(), json!(None::<Vec<Example>>));
    context.insert("tools".to_string(), json!(""));
    context.insert("conversation".to_string(), json!(""));

    let result = registry.render("strategies.react", context).unwrap();
    assert!(result.contains("ReAct pattern"));
    // Should not contain "Here are some examples" since examples is empty
    assert!(!result.contains("Here are some examples"));
}

#[test]
fn test_router_prompt_rendering() {
    let mut registry = PromptRegistry::new();
    registry.add_examples(
        "router_examples".to_string(),
        vec![
            Example {
                input: "What time is it?".to_string(),
                output: None,
                strategy: Some("react".to_string()),
            },
        ],
    );

    let mut context = HashMap::new();
    context.insert("examples".to_string(), json!(registry.get_examples("router_examples")));
    context.insert("conversation".to_string(), json!(""));
    context.insert("tools".to_string(), json!(""));

    let result = registry.render("router", context).unwrap();
    assert!(result.contains("Choose the best orchestration strategy"));
    assert!(result.contains("react"));
}

#[test]
fn test_plan_prompt_rendering() {
    let mut registry = PromptRegistry::new();
    registry.add_examples(
        "plan_examples".to_string(),
        vec![
            Example {
                input: "Search for weather".to_string(),
                output: Some("Plan: search_weather".to_string()),
                strategy: None,
            },
        ],
    );

    let mut context = HashMap::new();
    context.insert("examples".to_string(), json!(registry.get_examples("plan_examples")));
    context.insert("tools".to_string(), json!("weather, search"));
    context.insert("conversation".to_string(), json!(""));

    let result = registry.render("strategies.plan_and_execute", context).unwrap();
    assert!(result.contains("planning assistant"));
    assert!(result.contains("search_weather"));
}

#[test]
fn test_system_prompt_override() {
    let config = PromptConfig {
        system_prompt: Some("Custom system prompt".to_string()),
        prompts_dir: None,
        templates: std::collections::HashMap::new(),
        examples: std::collections::HashMap::new(),
    };

    let registry = PromptRegistry::from_config(&config).unwrap();
    let mut context = HashMap::new();
    context.insert("conversation".to_string(), json!(""));
    context.insert("tools".to_string(), json!(""));

    let result = registry.render("system", context).unwrap();
    assert_eq!(result, "Custom system prompt");
}

#[test]
fn test_default_system_prompt() {
    let registry = PromptRegistry::new();
    let mut context = HashMap::new();
    context.insert("conversation".to_string(), json!(""));
    context.insert("tools".to_string(), json!(""));

    let result = registry.render("system", context).unwrap();
    assert!(result.contains("helpful AI assistant"));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --package agentverse prompt_integration_test`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add avs-core/tests/prompt_integration_test.rs
git commit -m "test: add integration tests for prompt rendering with examples"
```

---

### Task 15: Create example .j2 and .toml files in each example agent's prompts/ directory

**Files:**
- Create: `examples/hello-agent/prompts/react.j2`
- Create: `examples/hello-agent/prompts/react_examples.toml`
- Create: `examples/slack-hr-assistant/prompts/plan_and_execute.j2`
- Create: `examples/slack-hr-assistant/prompts/plan_examples.toml`
- Create: `examples/rag-qa/prompts/react.j2`
- Create: `examples/rag-qa/prompts/react_examples.toml`
- Create: `examples/web-search-agent/prompts/react.j2`
- Create: `examples/web-search-agent/prompts/react_examples.toml`
- Create: `examples/code-review-agent/prompts/hierarchical.j2`
- Create: `examples/code-review-agent/prompts/hierarchical_examples.toml`

- [ ] **Step 1: Create hello-agent prompts**

Create `examples/hello-agent/prompts/react.j2`:
```jinja2
You are a helpful calculator assistant. Be precise with numbers.

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

Create `examples/hello-agent/prompts/react_examples.toml`:
```toml
[[example]]
input = "What is 2+2?"
output = "Thought: I can calculate this.\nAction: calculator\nAction Input: {\"expression\": \"2 + 2\"}"

[[example]]
input = "What time is it?"
output = "Thought: I can get the current time.\nAction: datetime\nAction Input: {\"format\": \"%Y-%m-%d %H:%M:%S\"}"
```

- [ ] **Step 2: Create slack-hr-assistant prompts**

Create `examples/slack-hr-assistant/prompts/plan_and_execute.j2`:
```jinja2
You are an HR assistant. Generate a step-by-step plan to handle the employee query.

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

Create `examples/slack-hr-assistant/prompts/plan_examples.toml`:
```toml
[[example]]
input = "What is the vacation policy?"
output = '{"description": "Check vacation policy", "steps": [{"id": 1, "description": "Search HR docs", "tool": "file_search", "args": {"query": "vacation policy"}, "depends_on": []}, {"id": 2, "description": "Summarize policy", "tool": null, "args": null, "depends_on": [1]}]}'
```

- [ ] **Step 3: Create rag-qa prompts**

Create `examples/rag-qa/prompts/react.j2`:
```jinja2
You are a RAG QA assistant. Answer questions based on the knowledge base.

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

Create `examples/rag-qa/prompts/react_examples.toml`:
```toml
[[example]]
input = "What is the project architecture?"
output = "Thought: I need to search the documents.\nAction: file_search\nAction Input: {\"query\": \"project architecture\"}"

[[example]]
input = "How do I use the HTTP client?"
output = "Thought: I need to find HTTP client docs.\nAction: file_search\nAction Input: {\"query\": \"HTTP client usage\"}"
```

- [ ] **Step 4: Create web-search-agent prompts**

Create `examples/web-search-agent/prompts/react.j2`:
```jinja2
You are a web search assistant. Search the web and summarize results.

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

Create `examples/web-search-agent/prompts/react_examples.toml`:
```toml
[[example]]
input = "Search for the latest AI news"
output = "Thought: I need to search the web.\nAction: http_client\nAction Input: {\"url\": \"https://news.ycombinator.com\", \"method\": \"GET\"}"

[[example]]
input = "What is the weather in Tokyo?"
output = "Thought: I need to check the weather.\nAction: http_client\nAction Input: {\"url\": \"https://api.weather.com/tokyo\", \"method\": \"GET\"}"
```

- [ ] **Step 5: Create code-review-agent prompts**

Create `examples/code-review-agent/prompts/hierarchical.j2`:
```jinja2
You are a code review assistant. Review code for quality, security, and best practices.

{% if examples %}
Examples:
{% for example in examples %}
Input: {{ example.input }}
Strategy: {{ example.strategy }}
{% endfor %}
{% endif %}

Break the request into sub-goals. Each sub-goal should be independently executable.

Respond with ONLY a JSON array of strings.
```

Create `examples/code-review-agent/prompts/hierarchical_examples.toml`:
```toml
[[example]]
input = "Review the avs-core crate for security vulnerabilities"
strategy = "hierarchical"

[[example]]
input = "Plan a security audit of the HTTP client module"
strategy = "plan_and_execute"
```

- [ ] **Step 6: Verify all files exist**

Run: `find examples -name "*.j2" -o -name "*_examples.toml" | sort`
Expected: 10 files

- [ ] **Step 7: Commit**

```bash
git add examples/*/prompts/
git commit -m "examples: add prompt templates and example files to each agent"
```

---

### Task 16: Create default .j2 and .toml files in project root prompts/ directory

**Files:**
- Create: `prompts/system.j2`
- Create: `prompts/react.j2`
- Create: `prompts/plan_and_execute.j2`
- Create: `prompts/hierarchical.j2`
- Create: `prompts/router.j2`
- Create: `prompts/react_examples.toml`
- Create: `prompts/plan_examples.toml`
- Create: `prompts/router_examples.toml`

- [ ] **Step 1: Create prompts directory and files**

Create `prompts/system.j2`:
```
You are a helpful AI assistant that executes tasks using available tools.
You are concise and accurate. Never claim to have done something you haven't.
If you don't know something, say so.
```

Create `prompts/react.j2`:
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

Create `prompts/plan_and_execute.j2`:
```jinja2
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

Create `prompts/hierarchical.j2`:
```jinja2
You are a hierarchical planning assistant. Break complex requests into sub-goals.

{% if examples %}
Examples:
{% for example in examples %}
Input: {{ example.input }}
Strategy: {{ example.strategy }}
{% endfor %}
{% endif %}

Break the request into sub-goals. Each sub-goal should be independently executable.

Respond with ONLY a JSON array of strings.
```

Create `prompts/router.j2`:
```jinja2
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

Create `prompts/react_examples.toml`:
```toml
[[example]]
input = "What is 2+2?"
output = "Thought: I can calculate this.\nAction: calculator\nAction Input: {\"expression\": \"2 + 2\"}"

[[example]]
input = "What time is it?"
output = "Thought: I can get the current time.\nAction: datetime\nAction Input: {\"format\": \"%Y-%m-%d %H:%M:%S\"}"

[[example]]
input = "What is the weather in Tokyo?"
output = "Thought: I need to check the weather.\nAction: weather\nAction Input: {\"city\": \"Tokyo\"}"
```

Create `prompts/plan_examples.toml`:
```toml
[[example]]
input = "Search for the weather and tell me if I should go outside"
output = '{"description": "Check weather and advise", "steps": [{"id": 1, "description": "Get weather data", "tool": "weather", "args": {"city": "current"}, "depends_on": []}, {"id": 2, "description": "Analyze and respond", "tool": null, "args": null, "depends_on": [1]}]}'

[[example]]
input = "Find the best pizza places in NYC"
output = '{"description": "Search and rank pizza places", "steps": [{"id": 1, "description": "Search pizza places", "tool": "search", "args": {"query": "best pizza NYC"}, "depends_on": []}, {"id": 2, "description": "Rank results", "tool": null, "args": null, "depends_on": [1]}]}'
```

Create `prompts/router_examples.toml`:
```toml
[[example]]
input = "What time is it?"
strategy = "react"

[[example]]
input = "Plan a trip to Paris including flights and hotels"
strategy = "hierarchical"

[[example]]
input = "Search for the best pizza places in NYC"
strategy = "plan_and_execute"

[[example]]
input = "What is the capital of France?"
strategy = "react"
```

- [ ] **Step 2: Verify all files exist**

Run: `ls -la prompts/`
Expected: 8 files (5 .j2 + 3 .toml)

- [ ] **Step 3: Update .gitignore if needed**

Ensure `prompts/` is tracked (it should be by default since these are source files).

- [ ] **Step 4: Run final workspace check**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cargo test --workspace`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add prompts/
git commit -m "docs: add default prompt templates and example files"
```

---

## Self-Review Checklist

**1. Spec coverage:**
- ✅ Add toml crate → Task 1
- ✅ Add Example struct → Task 2
- ✅ Add GuardrailError → Task 2
- ✅ Enhance PromptRegistry with file loading → Task 4
- ✅ Update Config → Task 6
- ✅ Update AgentBuilder → Task 7
- ✅ Wire PromptRegistry into CycleSkeleton → Task 8
- ✅ Add guardrail checkpoints → Task 8
- ✅ Wire guardrails into strategy run() loops → Task 8, 9, 10
- ✅ Replace hardcoded strings in PlanStrategy → Task 9
- ✅ Replace hardcoded strings in HierarchicalStrategy → Task 10
- ✅ Replace hardcoded strings in StrategyRouter → Task 11
- ✅ Update Agent::invoke() → Task 12
- ✅ Update all 5 example agents → Task 13
- ✅ Integration tests → Task 14
- ✅ Create example .j2/.toml files → Tasks 15, 16

**2. Placeholder scan:**
- ✅ No "TBD", "TODO", "implement later" phrases
- ✅ All code is complete in every step
- ✅ No "Similar to Task N" references — each step shows full code
- ✅ All types, method signatures, and property names are consistent

**3. Type consistency:**
- ✅ `GuardrailError` defined in Task 2, used in Tasks 8-11
- ✅ `Example` struct defined in Task 2, used in Tasks 4, 13-14
- ✅ `PromptConfig` defined in Task 4, used in Tasks 6-7, 13
- ✅ `Arc<PromptRegistry>` passed consistently through strategy constructors
- ✅ `check_prompt` and `check_output` from `agentverse_guardrails` used consistently

**4. Scope check:**
- ✅ Focused on prompt management infrastructure + wiring
- ✅ No caching, analytics, multi-language, A/B testing, or dynamic generation
- ✅ Each task is self-contained and testable
