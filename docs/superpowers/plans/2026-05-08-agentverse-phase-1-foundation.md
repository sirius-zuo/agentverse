# Phase 1: Foundation — avs-core

> **Goal:** Build the cargo workspace with avs-core crate: Agent struct, Builder, Config, Tool trait, ModelProvider trait, error types, and prompt registry.
> **Dependencies:** None (foundation layer)
> **Parallel:** Can run in parallel with nothing else — all other phases depend on this.

---

## File Structure

```
AgentVerse/
├── Cargo.toml                    # workspace root
├── .github/workflows/ci.yml      # CI config
├── avs-core/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs                # Public API re-exports
│   │   ├── agent.rs              # Agent struct + invoke()
│   │   ├── builder.rs            # AgentBuilder
│   │   ├── config.rs             # Config struct (serde Serialize/Deserialize)
│   │   ├── tool.rs               # SyncTool + AsyncTool traits
│   │   ├── model.rs              # ModelProvider trait
│   │   ├── error.rs              # AgentError + sub-types
│   │   ├── prompt.rs             # PromptRegistry + minijinja integration
│   │   ├── memory/
│   │   │   ├── mod.rs            # Memory trait + Message type
│   │   │   └── short_term.rs     # ShortTermMemory (Vec<Message> per user)
│   │   └── tracing/
│   │       ├── mod.rs            # NoopTracer + OtelTracer (feature-gated)
│   │       └── noop.rs           # Zero-overhead no-op tracer
│   └── tests/
│       ├── agent_test.rs
│       ├── builder_test.rs
│       ├── config_test.rs
│       └── error_test.rs
```

---

## Task 1: Workspace root + CI

**Files:**
- Create: `Cargo.toml`
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create workspace Cargo.toml**

```toml
[workspace]
members = [
    "avs-core",
    "avs-react",
    "avs-plan",
    "avs-router",
    "avs-memory",
    "avs-memory-lancedb",
    "avs-memory-pgvector",
    "avs-tools",
    "avs-mcp",
    "avs-guardrails",
    "avs-integration",
    "avs-server",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.75"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0"
async-trait = "0.1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
minijinja = "2.0"
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 2: Create CI workflow**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo check --all

  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all

  clippy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo clippy --all -- -D warnings

  fmt:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo fmt --all --check

  doc:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo doc --all --no-deps --document-private-items

  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cargo-audit 2>/dev/null || true
      - run: cargo audit || true  # non-blocking

  build-examples:
    runs-on: ubuntu-latest
    needs: [check]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo check --examples
```

- [ ] **Step 3: Verify workspace compiles**

Run: `cargo check --workspace`
Expected: PASS (all crates have empty src/lib.rs or are not yet created — skip non-existent for now)

Actually, run: `cargo check -p avs-core` (only core exists)
Expected: Will fail because avs-core doesn't exist yet. Proceed to Task 2.

---

## Task 2: avs-core crate skeleton

**Files:**
- Create: `avs-core/Cargo.toml`
- Create: `avs-core/src/lib.rs`

- [ ] **Step 1: Create avs-core Cargo.toml**

```toml
[package]
name = "agentverse"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
description = "Lightweight, extensible AI Agent framework"

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
async-trait.workspace = true
tracing.workspace = true
minijinja.workspace = true
reqwest.workspace = true
uuid.workspace = true
chrono.workspace = true

# Optional tracing
opentelemetry = { version = "0.22", optional = true }
opentelemetry-otlp = { version = "0.15", optional = true }

[features]
default = ["tracing"]
tracing = ["opentelemetry", "opentelemetry-otlp"]

[dev-dependencies]
tokio.workspace = true
httpmock = "0.7"
mockall = "0.13"
```

- [ ] **Step 2: Create lib.rs with public API re-exports**

```rust
//! AgentVerse: Lightweight, extensible AI Agent framework.
//!
//! ## Quick Start
//! ```
//! use agentverse::{Agent, AgentBuilder};
//!
//! // Build an agent programmatically
//! let agent = Agent::builder()
//!     .build();
//! ```

pub mod agent;
pub mod builder;
pub mod config;
pub mod error;
pub mod memory;
pub mod model;
pub mod prompt;
pub mod tool;
pub mod tracing;

// Public re-exports
pub use agent::Agent;
pub use builder::AgentBuilder;
pub use config::Config;
pub use error::{AgentError, ModelError, ToolError, ConfigError};
pub use memory::{Memory, Message, ShortTermMemory};
pub use model::ModelProvider;
pub use prompt::PromptRegistry;
pub use tool::{AsyncTool, SyncTool, ToolResult};
pub use tracing::{Tracer, NoopTracer};
```

- [ ] **Step 3: Create stub files for all modules**

```rust
// avs-core/src/agent.rs
pub struct Agent;
```

```rust
// avs-core/src/builder.rs
pub struct AgentBuilder;
```

```rust
// avs-core/src/config.rs
pub struct Config;
```

```rust
// avs-core/src/error.rs
pub enum AgentError {
    Model(ModelError),
    Tool(ToolError),
    Config(ConfigError),
}

#[derive(thiserror::Error, Debug)]
pub enum ModelError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(thiserror::Error, Debug)]
pub enum ToolError {
    #[error("Execution failed: {0}")]
    Execution(String),
    #[error("Not found: {0}")]
    NotFound(String),
}

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("Invalid config: {0}")]
    Invalid(String),
    #[error("Missing field: {0}")]
    Missing(String),
}
```

```rust
// avs-core/src/memory/mod.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

pub trait Memory {
    fn append(&mut self, message: Message);
    fn last_n(&self, n: usize) -> Vec<Message>;
    fn clear(&mut self);
}

mod short_term;
pub use short_term::ShortTermMemory;
```

```rust
// avs-core/src/memory/short_term.rs
use super::{Memory, Message};

pub struct ShortTermMemory {
    messages: Vec<Message>,
    max_messages: usize,
}

impl ShortTermMemory {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::with_capacity(max_messages),
            max_messages,
        }
    }
}

impl Memory for ShortTermMemory {
    fn append(&mut self, message: Message) {
        self.messages.push(message);
        if self.messages.len() > self.max_messages {
            self.messages.drain(0..self.messages.len() - self.max_messages);
        }
    }

    fn last_n(&self, n: usize) -> Vec<Message> {
        let start = self.messages.len().saturating_sub(n);
        self.messages[start..].to_vec()
    }

    fn clear(&mut self) {
        self.messages.clear();
    }
}
```

```rust
// avs-core/src/model.rs
use async_trait::async_trait;
use serde_json::Value;

use crate::error::ModelError;

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
```

```rust
// avs-core/src/tool.rs
use serde_json::Value;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Execution failed: {0}")]
    Execution(String),
    #[error("Not found: {0}")]
    NotFound(String),
}

pub type ToolResult = Result<Value, ToolError>;

pub trait SyncTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn execute(&self, args: Value) -> ToolResult;
}

pub trait AsyncTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    async fn execute(&self, args: Value) -> ToolResult;
}
```

```rust
// avs-core/src/prompt.rs
use minijinja::{Environment, Template};
use std::collections::HashMap;

pub struct PromptRegistry {
    env: Environment<'static>,
}

impl PromptRegistry {
    pub fn new() -> Self {
        Self {
            env: Environment::new(),
        }
    }

    pub fn add_template(&mut self, name: &str, template: &str) {
        self.env.add_template(name, template).unwrap();
    }

    pub fn render(&self, name: &str, context: HashMap<String, String>) -> Result<String, String> {
        let tmpl = self.env.get_template(name).map_err(|e| e.to_string())?;
        tmpl.render(context).map_err(|e| e.to_string())
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

```rust
// avs-core/src/tracing/mod.rs
mod noop;
pub use noop::NoopTracer;

#[cfg(feature = "tracing")]
mod otel;

#[cfg(feature = "tracing")]
pub use otel::OtelTracer;

pub trait Tracer: Send + Sync {
    fn span(&self, name: &str) -> Span;
}

pub struct Span;

impl Span {
    pub fn set_attribute(self, _key: &str, _value: &str) {}
}

// Default: use NoopTracer when tracing feature is disabled
#[cfg(not(feature = "tracing"))]
pub type DefaultTracer = NoopTracer;

#[cfg(feature = "tracing")]
pub type DefaultTracer = OtelTracer;
```

```rust
// avs-core/src/tracing/noop.rs
use super::{Span, Tracer};

pub struct NoopTracer;

impl Tracer for NoopTracer {
    fn span(&self, _name: &str) -> Span {
        Span
    }
}
```

```rust
// avs-core/src/agent.rs
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AgentError;
use crate::memory::{Memory, Message, ShortTermMemory};
use crate::model::ModelProvider;
use crate::prompt::PromptRegistry;
use crate::tool::{SyncTool, ToolResult};
use crate::tracing::Tracer;

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

    pub fn from_config(config: Config) -> Result<Self, AgentError> {
        // Validation happens in Config::validate()
        config.validate()?;

        Ok(Self {
            config,
            memory: Arc::new(RwLock::new(ShortTermMemory::new(100))),
            prompt_registry: PromptRegistry::new(),
            tracer: Box::new(crate::tracing::DefaultTracer),
        })
    }

    pub async fn invoke(&self, user_id: &str, input: &str) -> Result<String, AgentError> {
        let mut memory = self.memory.write().await;
        memory.append(Message {
            role: crate::memory::MessageRole::User,
            content: input.to_string(),
        });
        drop(memory);

        // TODO: Strategy loop will be implemented in avs-react
        // For now, return a placeholder
        Ok(format!("Processed: {}", input))
    }
}
```

```rust
// avs-core/src/builder.rs
use crate::config::Config;
use crate::error::AgentError;
use crate::model::ModelProvider;
use crate::tool::SyncTool;

pub struct AgentBuilder {
    model: Option<Box<dyn ModelProvider>>,
    tools: Vec<Box<dyn SyncTool>>,
    max_messages: usize,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model: None,
            tools: Vec::new(),
            max_messages: 100,
        }
    }

    pub fn model(mut self, model: impl ModelProvider + 'static) -> Self {
        self.model = Some(Box::new(model));
        self
    }

    pub fn tool(mut self, tool: impl SyncTool + 'static) -> Self {
        self.tools.push(Box::new(tool));
        self
    }

    pub fn max_messages(mut self, max: usize) -> Self {
        self.max_messages = max;
        self
    }

    pub fn build(self) -> Result<crate::agent::Agent, AgentError> {
        if self.model.is_none() {
            return Err(AgentError::Config(ConfigError::Missing(
                "model is required".to_string(),
            )));
        }

        let config = Config {
            model_api_key: String::new(), // simplified
            model_name: String::new(),
            max_messages: self.max_messages,
            tools: vec![],
        };

        crate::agent::Agent::from_config(config)
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
```

```rust
// avs-core/src/config.rs
use serde::{Deserialize, Serialize};

use crate::error::{AgentError, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model_api_key: String,
    pub model_name: String,
    pub max_messages: usize,
    #[serde(default)]
    pub tools: Vec<String>,
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

> **Note:** Config needs `serde_yaml` dependency. Add to avs-core Cargo.toml:
> ```toml
> serde_yaml = "0.9"
> ```

- [ ] **Step 4: Add serde_yaml dependency to avs-core/Cargo.toml**

Edit `avs-core/Cargo.toml`:
```toml
[dependencies]
...
serde_yaml = "0.9"
```

- [ ] **Step 5: Verify avs-core compiles**

Run: `cargo check -p agentverse`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml avs-core/
git commit -m "feat: add avs-core workspace and crate skeleton"
```

---

## Task 3: Core tests

**Files:**
- Create: `avs-core/tests/agent_test.rs`
- Create: `avs-core/tests/builder_test.rs`
- Create: `avs-core/tests/config_test.rs`
- Create: `avs-core/tests/error_test.rs`

- [ ] **Step 1: Create error tests**

```rust
// avs-core/tests/error_test.rs
use agentverse::{AgentError, ModelError, ToolError, ConfigError};

#[test]
fn test_error_display() {
    let err = ModelError::ApiError("401 Unauthorized".to_string());
    assert_eq!(err.to_string(), "API error: 401 Unauthorized");

    let err = ToolError::Execution("file not found".to_string());
    assert_eq!(err.to_string(), "Execution failed: file not found");
}

#[test]
fn test_agent_error_from_model() {
    let model_err = ModelError::Timeout("gpt-4".to_string());
    let agent_err = AgentError::Model(model_err);
    assert!(matches!(agent_err, AgentError::Model(_)));
}
```

- [ ] **Step 2: Create config tests**

```rust
// avs-core/tests/config_test.rs
use agentverse::Config;

#[test]
fn test_config_validation_missing_key() {
    let config = Config {
        model_api_key: String::new(),
        model_name: "gpt-4".to_string(),
        max_messages: 100,
        tools: vec![],
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_missing_name() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: String::new(),
        max_messages: 100,
        tools: vec![],
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_valid() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 100,
        tools: vec![],
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_serialization() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 200,
        tools: vec!["search".to_string()],
    };
    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("model_name: gpt-4"));
    let deserialized: Config = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(deserialized.model_name, "gpt-4");
}
```

- [ ] **Step 3: Create builder tests**

```rust
// avs-core/tests/builder_test.rs
use agentverse::AgentBuilder;

#[test]
fn test_builder_requires_model() {
    let builder = AgentBuilder::new();
    let result = builder.build();
    assert!(result.is_err());
}
```

- [ ] **Step 4: Create agent tests (basic, without strategy)**

```rust
// avs-core/tests/agent_test.rs
use agentverse::{Agent, Config};

#[test]
fn test_agent_from_config_valid() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
    };
    let agent = Agent::from_config(config);
    assert!(agent.is_ok());
}

#[tokio::test]
async fn test_agent_invoke_placeholder() {
    let config = Config {
        model_api_key: "sk-xxx".to_string(),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
    };
    let agent = Agent::from_config(config).unwrap();
    let result = agent.invoke("user1", "hello").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Processed: hello");
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -p agentverse`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add avs-core/tests/
git commit -m "test: add core unit tests"
```

---

## Task 4: OpenAICompatibleModelProvider implementation

**Files:**
- Create: `avs-core/src/model/openai_compatible.rs`

- [ ] **Step 1: Create OpenAICompatible provider**

```rust
// avs-core/src/model/openai_compatible.rs
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::error::ModelError;
use crate::model::ToolDefinition;

#[derive(Debug, Clone)]
pub struct OpenAICompatible {
    client: Client,
    api_base: String,
    model_name: String,
    api_key: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
}

#[derive(Debug, Clone, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatTool {
    r#type: String,
    function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
struct FunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ToolCall {
    function: FunctionCall,
}

#[derive(Debug, Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

impl OpenAICompatible {
    pub fn new(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_base: api_base.to_string(),
            model_name: model_name.to_string(),
            api_key: api_key.to_string(),
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.api_base)
    }
}

#[async_trait]
impl ModelProvider for OpenAICompatible {
    async fn generate(
        &self,
        prompt: &str,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError> {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }];

        let chat_tools = tools.map(|t| {
            t.into_iter()
                .map(|tool| ChatTool {
                    r#type: "function".to_string(),
                    function: FunctionDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    },
                })
                .collect()
        });

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages,
            tools: chat_tools,
        };

        let response = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ModelError::ApiError(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ModelError::ApiError(e.to_string()))?;

        if !status.is_success() {
            return Err(ModelError::ApiError(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let chat_response: ChatResponse = serde_json::from_str(&body)
            .map_err(|e| ModelError::InvalidResponse(e.to_string()))?;

        chat_response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| ModelError::InvalidResponse("No content in response".to_string()))
    }
}
```

- [ ] **Step 2: Update model.rs to re-export**

Edit `avs-core/src/model.rs`:
```rust
use async_trait::async_trait;
use serde_json::Value;

use crate::error::ModelError;

mod openai_compatible;
pub use openai_compatible::OpenAICompatible;

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
```

- [ ] **Step 3: Add reqwest feature for json to avs-core/Cargo.toml** (already present)

- [ ] **Step 4: Create test for OpenAICompatible**

```rust
// avs-core/tests/openai_test.rs
use agentverse::model::{ModelProvider, OpenAICompatible, ToolDefinition};
use httpmock::prelude::*;

#[tokio::test]
async fn test_openai_compatible_generate() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method("POST")
            .path("/chat/completions")
            .header("Authorization", "Bearer test-key")
            .json_body(json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "hello"}]
            }));
        then.status(200)
            .json_body(json!({
                "choices": [{
                    "message": {
                        "content": "Hello! How can I help you?"
                    }
                }]
            }));
    });

    let model = OpenAICompatible::new(
        &server.base_url(),
        "test-model",
        "test-key",
    );

    let result = model.generate("hello", None).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Hello! How can I help you?");

    mock.assert();
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p agentverse`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add avs-core/src/model/
git commit -m "feat: add OpenAICompatible model provider"
```

---

## Task 5: PromptRegistry with default templates

**Files:**
- Modify: `avs-core/src/prompt.rs` (add default templates)

- [ ] **Step 1: Add default ReAct template**

```rust
// avs-core/src/prompt.rs
use minijinja::{Environment, Template};
use std::collections::HashMap;

const DEFAULT_REACT_TEMPLATE: &str = r#"
You are a helpful assistant. Think step by step.

Current conversation:
{% for message in conversation %}
{{ message.role }}: {{ message.content }}
{% endfor %}

Available tools:
{% for tool in tools %}
- {{ tool.name }}: {{ tool.description }}
{% endfor %}

Respond in the following format:
Thought: [your reasoning]
Action: [tool name]
Action Input: [tool arguments as JSON]

Or if you have the final answer:
Thought: [your reasoning]
Answer: [final answer]
"#;

pub struct PromptRegistry {
    env: Environment<'static>,
}

impl PromptRegistry {
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.add_template("react", DEFAULT_REACT_TEMPLATE).unwrap();
        Self { env }
    }

    pub fn add_template(&mut self, name: &str, template: &str) {
        self.env.add_template(name, template).unwrap();
    }

    pub fn render(
        &self,
        name: &str,
        context: HashMap<String, String>,
    ) -> Result<String, String> {
        let tmpl = self.env.get_template(name).map_err(|e| e.to_string())?;
        // For now, use a simple string context
        let mut ctx = minijinja::Context::new();
        for (k, v) in context {
            ctx.insert(&k, v);
        }
        tmpl.render(ctx).map_err(|e| e.to_string())
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: Create prompt tests**

```rust
// avs-core/tests/prompt_test.rs
use agentverse::PromptRegistry;
use std::collections::HashMap;

#[test]
fn test_prompt_registry_render() {
    let registry = PromptRegistry::new();
    let mut ctx = HashMap::new();
    ctx.insert("name".to_string(), "AgentVerse".to_string());
    let result = registry.render("react", ctx);
    assert!(result.is_ok());
    assert!(result.unwrap().contains("helpful assistant"));
}

#[test]
fn test_prompt_registry_unknown_template() {
    let registry = PromptRegistry::new();
    let result = registry.render("nonexistent", HashMap::new());
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p agentverse`
Expected: All tests PASS

- [ ] **Step 4: Commit**

```bash
git add avs-core/src/prompt.rs avs-core/tests/prompt_test.rs
git commit -m "feat: add PromptRegistry with default ReAct template"
```

---

## Task 6: Final verification + integration test

- [ ] **Step 1: Run full workspace check**

Run: `cargo check -p agentverse`
Expected: PASS

- [ ] **Step 2: Run all tests**

Run: `cargo test -p agentverse -- --nocapture`
Expected: All tests PASS

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p agentverse -- -D warnings`
Expected: PASS (fix any warnings)

- [ ] **Step 4: Commit final**

```bash
git add -A
git commit -m "chore: finalize avs-core foundation"
```

---

## Phase 1 Acceptance Criteria

- [ ] Workspace compiles with `cargo check -p agentverse`
- [ ] All unit tests pass with `cargo test -p agentverse`
- [ ] Clippy passes with no warnings
- [ ] `Agent::builder()` and `Agent::from_config()` work
- [ ] `OpenAICompatible` model provider compiles and tests pass
- [ ] `PromptRegistry` renders templates
- [ ] `ShortTermMemory` stores and retrieves messages
- [ ] Error types are properly structured and displayable
- [ ] Config serializes/deserializes to YAML
- [ ] CI workflow exists (even if not yet triggered)

## Dependencies for Next Phases

This phase must complete before:
- Phase 2 (Strategies) — depends on Agent, Tool, ModelProvider, Memory, PromptRegistry
- Phase 3 (Memory backends) — depends on Memory trait
- Phase 4 (Tools) — depends on Tool trait
- Phase 5 (Guardrails) — depends on AgentError, Tool trait
- Phase 6 (Server) — depends on all above

## Estimated Effort

~4-6 hours for a Rust-experienced developer. All tasks are sequential within the phase (no parallelism needed since it's a single crate).
