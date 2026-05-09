# Phase 2: Orchestration Strategies

> **Goal:** Implement ReAct, Plan-and-Execute, and Hierarchical Planning strategies with the fixed cycle skeleton and phase-aware context.
> **Dependencies:** Phase 1 (avs-core) must be complete
> **Parallel:** avs-react, avs-plan, and avs-router can be developed in parallel once avs-core is stable

---

## File Structure

```
AgentVerse/
├── avs-react/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── strategy.rs       # ReActStrategy impl
│   │   ├── cycle.rs          # Shared cycle skeleton
│   │   └── steps.rs          # ReAct-specific step logic (think/act/answer)
│   └── tests/
│       └── react_test.rs
│
├── avs-plan/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── plan_strategy.rs  # Plan-and-Execute
│   │   ├── hierarchical.rs   # Hierarchical Planning
│   │   └── planner.rs        # Shared planning utilities
│   └── tests/
│       └── plan_test.rs
│
└── avs-router/
    ├── Cargo.toml
    ├── src/
    │   ├── lib.rs
    │   └── router.rs         # StrategyRouter (LLM-based)
    └── tests/
        └── router_test.rs
```

---

## Task 1: avs-react crate — Shared Cycle Skeleton

**Files:**
- Create: `avs-react/Cargo.toml`
- Create: `avs-react/src/lib.rs`
- Create: `avs-react/src/cycle.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "agentverse-react"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
```

- [ ] **Step 2: Create lib.rs**

```rust
//! ReAct orchestration strategy for AgentVerse.
//!
//! Implements the ReAct pattern: Think → Act → Observe → Think...

pub mod cycle;
pub mod strategy;
pub mod steps;

pub use strategy::ReActStrategy;
```

- [ ] **Step 3: Create the shared cycle skeleton**

This is the core of Phase 2. The cycle skeleton is shared by ALL strategies (ReAct, Plan, Hierarchical). It handles the loop structure; each strategy only implements `step()`.

```rust
// avs-react/src/cycle.rs
use agentverse::{
    AgentError, Message, Memory, ModelProvider, PromptRegistry, SyncTool, ToolResult, Tracer,
};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

/// The fixed cycle skeleton that all strategies share.
/// Each strategy only needs to implement `step()` to decide what happens next.
pub struct CycleSkeleton<P, M, MT>
where
    P: ModelProvider,
    M: Memory,
    MT: SyncTool,
{
    prompt_registry: PromptRegistry,
    model: P,
    tools: Vec<MT>,
    memory: M,
    tracer: Box<dyn agentverse::Tracer>,
    max_iterations: usize,
    current_iteration: usize,
}

/// Represents the strategy's decision for the next action.
pub enum CycleAction {
    /// LLM said "think" — continue the loop
    Continue {
        thought: String,
    },
    /// LLM decided to call a tool
    ToolCall {
        tool_name: String,
        args: Value,
    },
    /// LLM provided a final answer
    Done {
        answer: String,
    },
    /// LLM indicated an error
    Error {
        message: String,
    },
}

impl<P, M, MT> CycleSkeleton<P, M, MT>
where
    P: ModelProvider,
    M: Memory,
    MT: SyncTool,
{
    pub fn new(
        prompt_registry: PromptRegistry,
        model: P,
        tools: Vec<MT>,
        memory: M,
        tracer: Box<dyn agentverse::Tracer>,
        max_iterations: usize,
    ) -> Self {
        Self {
            prompt_registry,
            model,
            tools,
            memory,
            tracer,
            max_iterations,
            current_iteration: 0,
        }
    }

    /// Run the strategy loop. Each strategy provides its own `step()` implementation.
    pub fn run<F>(
        &mut self,
        initial_message: String,
        mut step: F,
    ) -> Result<String, AgentError>
    where
        F: FnMut(&mut Self) -> Result<CycleAction, AgentError>,
    {
        // Append initial message to memory
        self.memory.append(Message {
            role: agentverse::MessageRole::User,
            content: initial_message.clone(),
        });

        while self.current_iteration < self.max_iterations {
            self.current_iteration += 1;
            debug!(iteration = self.current_iteration, "Running strategy step");

            let action = step(self)?;

            match action {
                CycleAction::Continue { thought } => {
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                    info!(iteration = self.current_iteration, "Thought only, continuing");
                }
                CycleAction::ToolCall {
                    tool_name,
                    args,
                } => {
                    let result = self.execute_tool(&tool_name, args)?;
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Tool,
                        content: format!("Tool: {}\nResult: {}", tool_name, result),
                    });
                    info!(iteration = self.current_iteration, tool = tool_name, "Tool executed");
                }
                CycleAction::Done { answer } => {
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Assistant,
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

    /// Execute a tool by name with the given arguments.
    fn execute_tool(&self, tool_name: &str, args: Value) -> Result<String, AgentError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| {
                AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string()))
            })?;

        let result = tool.execute(args).map_err(|e| AgentError::Tool(e))?;
        Ok(result.to_string())
    }

    /// Build the prompt for the LLM.
    fn build_prompt(&self) -> Result<String, AgentError> {
        let last_messages = self.memory.last_n(20);
        let mut context = HashMap::new();

        let conversation = last_messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        context.insert("conversation".to_string(), conversation);

        let tools_desc = self
            .tools
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");
        context.insert("tools".to_string(), tools_desc);

        self.prompt_registry
            .render("react", context)
            .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e)))
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p agentverse-react`
Expected: Will fail because the `agentverse::Tracer` trait and some types need to be exported. Fix in Task 2.

- [ ] **Step 5: Commit initial structure**

```bash
git add avs-react/
git commit -m "feat: add avs-react crate with cycle skeleton"
```

---

## Task 2: avs-react — ReAct Step Logic

**Files:**
- Create: `avs-react/src/steps.rs`
- Create: `avs-react/src/strategy.rs`

- [ ] **Step 1: Create ReAct step logic**

```rust
// avs-react/src/steps.rs
use super::cycle::{CycleAction, CycleSkeleton};
use agentverse::{AgentError, ToolDefinition};
use serde_json::Value;

/// Parse the LLM response to extract thought, action, and answer.
/// Expected formats:
///   - "Thought: xxx\nAction: tool_name\nAction Input: {...}"
///   - "Thought: xxx\nAnswer: final answer"
pub fn parse_response(response: &str) -> CycleAction {
    // Check for answer first
    if let Some(answer_pos) = response.to_lowercase().find("answer:") {
        let answer = response[answer_pos..].trim().trim_start_matches(|c: char| c == 'A' || c == 'a').trim().to_string();
        return CycleAction::Done { answer };
    }

    // Check for action
    if let Some(action_pos) = response.to_lowercase().find("action:") {
        let action_part = &response[action_pos..];
        let tool_name = extract_tool_name(action_part);
        let args = extract_args(response, action_pos);

        return CycleAction::ToolCall {
            tool_name,
            args,
        };
    }

    // Just a thought
    CycleAction::Continue {
        thought: response.to_string(),
    }
}

fn extract_tool_name(response: &str) -> String {
    // Find the line after "Action:"
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("action:") {
            let after = &trimmed["Action:".len()..].trim();
            // Try to parse as JSON (tool name might be a simple string)
            return after.to_string();
        }
    }
    "unknown".to_string()
}

fn extract_args(response: &str, action_pos: usize) -> Value {
    // Look for "Action Input:" line
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("action input:") {
            let json_str = &trimmed["Action Input:".len()..].trim();
            if let Ok(value) = serde_json::from_str(json_str) {
                return value;
            }
        }
    }
    Value::Null
}

/// The ReAct step implementation.
/// This is the `step` function passed to CycleSkeleton::run().
pub async fn react_step<P, M, MT>(
    cycle: &mut CycleSkeleton<P, M, MT>,
) -> Result<CycleAction, AgentError>
where
    P: agentverse::ModelProvider,
    M: agentverse::Memory,
    MT: agentverse::SyncTool,
{
    let prompt = cycle.build_prompt()?;

    // Build tool definitions for the LLM
    let tool_defs: Vec<ToolDefinition> = cycle
        .tools
        .iter()
        .map(|t| ToolDefinition {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters(),
        })
        .collect();

    let response = cycle.model.generate(&prompt, Some(tool_defs)).await?;

    Ok(parse_response(&response))
}
```

- [ ] **Step 2: Create ReActStrategy wrapper**

```rust
// avs-react/src/strategy.rs
use agentverse::{AgentError, Memory, PromptRegistry, SyncTool, Tracer};

use super::cycle::{CycleAction, CycleSkeleton};
use super::steps::react_step;
use crate::steps::parse_response;

use async_trait::async_trait;

/// The high-level ReAct strategy interface.
/// Users interact with this, not CycleSkeleton directly.
pub struct ReActStrategy<P, M, MT>
where
    P: agentverse::ModelProvider,
    M: agentverse::Memory,
    MT: agentverse::SyncTool,
{
    skeleton: CycleSkeleton<P, M, MT>,
}

impl<P, M, MT> ReActStrategy<P, M, MT>
where
    P: agentverse::ModelProvider,
    M: agentverse::Memory,
    MT: agentverse::SyncTool,
{
    pub fn new(
        prompt_registry: PromptRegistry,
        model: P,
        tools: Vec<MT>,
        memory: M,
        tracer: Box<dyn Tracer>,
        max_iterations: usize,
    ) -> Self {
        Self {
            skeleton: CycleSkeleton::new(prompt_registry, model, tools, memory, tracer, max_iterations),
        }
    }

    /// Execute the ReAct loop.
    pub async fn run(&mut self, input: String) -> Result<String, AgentError> {
        let mut skeleton = std::mem::replace(
            &mut self.skeleton,
            CycleSkeleton::new(
                PromptRegistry::new(),
                // We need to clone/recreate model — this is a design limitation
                // In practice, model should be Clone or wrapped in Arc
                unimplemented!("Model must be Clone for re-creation"),
                vec![],
                // Memory must be Clone or wrapped in Arc<Mutex>
                unimplemented!("Memory must be Clone for re-creation"),
                Box::new(agentverse::NoopTracer),
                0,
            ),
        );

        // Re-attach the original components
        // This is getting messy. Let me refactor.

        Ok(String::new())
    }
}
```

> **Wait** — I realize there's a design issue here. The `CycleSkeleton` takes ownership of model, tools, memory. But we want to call `run()` multiple times. Let me fix this by wrapping in `Arc`.

Let me rewrite the cycle and strategy to use `Arc`:

```rust
// avs-react/src/cycle.rs (REWRITTEN with Arc)
use agentverse::{
    AgentError, Message, Memory, ModelProvider, PromptRegistry, SyncTool, ToolResult,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

pub struct CycleSkeleton<P, M, MT>
where
    P: ModelProvider,
    M: Memory,
    MT: SyncTool,
{
    prompt_registry: Arc<PromptRegistry>,
    model: Arc<P>,
    tools: Vec<MT>,
    memory: Arc<M>,
    max_iterations: usize,
    current_iteration: usize,
}

pub enum CycleAction {
    Continue { thought: String },
    ToolCall { tool_name: String, args: Value },
    Done { answer: String },
    Error { message: String },
}

impl<P, M, MT> CycleSkeleton<P, M, MT>
where
    P: ModelProvider,
    M: Memory,
    MT: SyncTool,
{
    pub fn new(
        prompt_registry: Arc<PromptRegistry>,
        model: Arc<P>,
        tools: Vec<MT>,
        memory: Arc<M>,
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

    pub fn run<F>(&mut self, initial_message: String, mut step: F) -> Result<String, AgentError>
    where
        F: FnMut(&mut Self) -> Result<CycleAction, AgentError>,
    {
        self.memory.append(Message {
            role: agentverse::MessageRole::User,
            content: initial_message.clone(),
        });

        while self.current_iteration < self.max_iterations {
            self.current_iteration += 1;
            debug!(iteration = self.current_iteration, "Running strategy step");

            let action = step(self)?;

            match action {
                CycleAction::Continue { thought } => {
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                    info!(iteration = self.current_iteration, "Thought only, continuing");
                }
                CycleAction::ToolCall {
                    tool_name,
                    args,
                } => {
                    let result = self.execute_tool(&tool_name, args)?;
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Tool,
                        content: format!("Tool: {}\nResult: {}", tool_name, result),
                    });
                    info!(iteration = self.current_iteration, tool = tool_name, "Tool executed");
                }
                CycleAction::Done { answer } => {
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Assistant,
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

    fn execute_tool(&self, tool_name: &str, args: Value) -> Result<String, AgentError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string())))?;

        let result = tool.execute(args).map_err(|e| AgentError::Tool(e))?;
        Ok(result.to_string())
    }

    fn build_prompt(&self) -> Result<String, AgentError> {
        let last_messages = self.memory.lock().unwrap().last_n(20);
        let mut context = HashMap::new();

        let conversation = last_messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        context.insert("conversation".to_string(), conversation);

        let tools_desc = self
            .tools
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");
        context.insert("tools".to_string(), tools_desc);

        self.prompt_registry.render("react", context)
            .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e)))
    }
}
```

> **Wait** — `Memory` trait is not `Send + Sync`. I need to make it `Sync + Send` and use `Arc<RwLock<dyn Memory>>` in the skeleton. But the spec said we'd use `Arc<RwLock<AgentState>>` at the Agent level, not per-strategy. Let me keep the strategy simple: it takes `Arc<dyn Memory>` and the trait must be `Send + Sync`.

Let me fix the Memory trait in avs-core first, then continue with the strategy.

Actually, let me simplify. The cycle skeleton in avs-react doesn't need to own Memory — it should receive a reference. Let me restructure:

```rust
// avs-react/src/cycle.rs (FINAL VERSION)
use agentverse::{AgentError, Message, ModelProvider, PromptRegistry, SyncTool};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

pub struct CycleSkeleton<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    prompt_registry: Arc<PromptRegistry>,
    model: Arc<P>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<M>,
    max_iterations: usize,
    current_iteration: usize,
}

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
        memory: Arc<M>,
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

    pub fn run<F>(&mut self, initial_message: String, mut step: F) -> Result<String, AgentError>
    where
        F: FnMut(&mut Self) -> Result<CycleAction, AgentError>,
    {
        self.memory.append(Message {
            role: agentverse::MessageRole::User,
            content: initial_message.clone(),
        });

        while self.current_iteration < self.max_iterations {
            self.current_iteration += 1;
            debug!(iteration = self.current_iteration, "Running strategy step");

            let action = step(self)?;

            match action {
                CycleAction::Continue { thought } => {
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                }
                CycleAction::ToolCall { tool_name, args } => {
                    let result = self.execute_tool(&tool_name, args)?;
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Tool,
                        content: format!("Tool: {}\nResult: {}", tool_name, result),
                    });
                }
                CycleAction::Done { answer } => {
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: answer.clone(),
                    });
                    return Ok(answer);
                }
                CycleAction::Error { message } => {
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

    fn execute_tool(&self, tool_name: &str, args: Value) -> Result<String, AgentError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string())))?;

        let result = tool.execute(args).map_err(|e| AgentError::Tool(e))?;
        Ok(result.to_string())
    }

    fn build_prompt(&self) -> Result<String, AgentError> {
        // Simplified: just get last 20 messages
        let conversation = self.memory.last_n(20)
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let mut context = HashMap::new();
        context.insert("conversation".to_string(), conversation);

        let tools_desc = self.tools
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");
        context.insert("tools".to_string(), tools_desc);

        self.prompt_registry
            .render("react", context)
            .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e)))
    }
}
```

OK this is getting complex. Let me simplify the plan — the actual code will be worked out during implementation. Let me write the plan at a higher level with key code snippets.

Let me rewrite this phase plan more concisely, focusing on the key code and leaving implementation details for the actual coding phase.

<tool_call>
<function=write>
<parameter=content>
# Phase 2: Orchestration Strategies

> **Goal:** Implement ReAct, Plan-and-Execute, and Hierarchical Planning strategies.
> **Dependencies:** Phase 1 (avs-core) must be complete
> **Parallel:** All 3 crates can develop in parallel once avs-core is stable

---

## Overview

The strategy layer uses a **fixed cycle skeleton** shared across all strategies. Each strategy only implements `step()` to decide the next action.

```
CycleSkeleton::run() {
    loop {
        let action = strategy.step(self)?;
        match action {
            Continue → append thought to memory, continue loop
            ToolCall → execute tool, append result, continue loop
            Done     → append answer, return
            Error    → return error
        }
    }
}
```

## File Structure

```
avs-react/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── cycle.rs          # Shared CycleSkeleton<P, M>
│   ├── react.rs          # ReActStrategy + step logic
│   └── parse.rs          # LLM response parser (thought/action/answer)
└── tests/

avs-plan/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── plan.rs           # Plan-and-Execute strategy
│   ├── hierarchical.rs   # Hierarchical Planning strategy
│   └── planner.rs        # Shared planning utilities (decompose, validate)
└── tests/

avs-router/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   └── router.rs         # StrategyRouter: LLM-based dynamic routing
└── tests/
```

---

## Task 1: avs-react — Cycle Skeleton + ReAct

**Files:**
- Create: `avs-react/Cargo.toml`
- Create: `avs-react/src/lib.rs`
- Create: `avs-react/src/cycle.rs`
- Create: `avs-react/src/react.rs`
- Create: `avs-react/src/parse.rs`
- Create: `avs-react/tests/react_test.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-react"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
async-trait.workspace = true
```

- [ ] **Step 2: cycle.rs — Shared cycle skeleton**

```rust
// avs-react/src/cycle.rs
use agentverse::{AgentError, Message, ModelProvider, PromptRegistry, SyncTool};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub struct CycleSkeleton<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    prompt_registry: Arc<PromptRegistry>,
    model: Arc<P>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<M>,
    max_iterations: usize,
    current_iteration: usize,
}

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
        memory: Arc<M>,
        max_iterations: usize,
    ) -> Self {
        Self {
            prompt_registry, model, tools, memory,
            max_iterations, current_iteration: 0,
        }
    }

    pub fn run<F>(&mut self, initial_message: String, mut step: F) -> Result<String, AgentError>
    where
        F: FnMut(&mut Self) -> Result<CycleAction, AgentError>,
    {
        self.memory.append(Message {
            role: agentverse::MessageRole::User,
            content: initial_message,
        });

        while self.current_iteration < self.max_iterations {
            self.current_iteration += 1;

            let action = step(self)?;
            match action {
                CycleAction::Continue { thought } => {
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                }
                CycleAction::ToolCall { tool_name, args } => {
                    let result = self.execute_tool(&tool_name, args)?;
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Tool,
                        content: format!("Tool: {}\nResult: {}", tool_name, result),
                    });
                }
                CycleAction::Done { answer } => {
                    self.memory.append(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: answer.clone(),
                    });
                    return Ok(answer);
                }
                CycleAction::Error { message } => {
                    return Err(AgentError::Model(agentverse::ModelError::InvalidResponse(message)));
                }
            }
        }

        Err(AgentError::Model(agentverse::ModelError::Timeout(format!(
            "Max iterations ({}) reached", self.max_iterations
        ))))
    }

    fn execute_tool(&self, tool_name: &str, args: Value) -> Result<String, AgentError> {
        let tool = self.tools.iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string())))?;
        let result = tool.execute(args).map_err(|e| AgentError::Tool(e))?;
        Ok(result.to_string())
    }

    pub fn build_prompt(&self) -> Result<String, AgentError> {
        let conversation = self.memory.last_n(20)
            .iter().map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>().join("\n");

        let mut context = HashMap::new();
        context.insert("conversation".to_string(), conversation);
        context.insert("tools".to_string(), self.tools.iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>().join("\n"));

        self.prompt_registry.render("react", context)
            .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e)))
    }
}
```

- [ ] **Step 3: parse.rs — LLM response parser**

```rust
// avs-react/src/parse.rs
use super::cycle::CycleAction;
use serde_json::Value;

pub fn parse_response(response: &str) -> CycleAction {
    let lower = response.to_lowercase();

    // Check for Answer: first
    if let Some(pos) = lower.find("answer:") {
        let answer = response[pos + 7..].trim().to_string();
        return CycleAction::Done { answer };
    }

    // Check for Action:
    if let Some(pos) = lower.find("action:") {
        let tool_name = extract_tool_name(response, pos);
        let args = extract_args(response, pos);
        return CycleAction::ToolCall { tool_name, args };
    }

    // Just a thought
    CycleAction::Continue { thought: response.to_string() }
}

fn extract_tool_name(response: &str, action_pos: usize) -> String {
    for line in response.lines().skip_while(|l| !l.trim().to_lowercase().starts_with("action:")) {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("action:") {
            return trimmed["Action:".len()..].trim().to_string();
        }
    }
    "unknown".to_string()
}

fn extract_args(response: &str, action_pos: usize) -> Value {
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("action input:") {
            let json_str = &trimmed["Action Input:".len()..].trim();
            if let Ok(v) = serde_json::from_str(json_str) {
                return v;
            }
        }
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_answer() {
        let r = parse_response("Thought: I know the answer.\nAnswer: 42");
        match r {
            CycleAction::Done { answer } => assert_eq!(answer, "42"),
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_tool_call() {
        let r = parse_response("Thought: Let me search.\nAction: file_search\nAction Input: {\"path\": \".\"}");
        match r {
            CycleAction::ToolCall { tool_name, args } => {
                assert_eq!(tool_name, "file_search");
                assert_eq!(args["path"], ".");
            }
            _ => panic!("Expected ToolCall"),
        }
    }
}
```

- [ ] **Step 4: react.rs — ReActStrategy**

```rust
// avs-react/src/react.rs
use super::cycle::{CycleAction, CycleSkeleton};
use super::parse::parse_response;
use agentverse::{AgentError, Memory, ModelProvider, PromptRegistry, SyncTool, ToolDefinition};
use std::sync::Arc;

pub struct ReActStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    skeleton: CycleSkeleton<P, M>,
}

impl<P, M> ReActStrategy<P, M>
where
    P: ModelProvider + Clone,
    M: agentverse::Memory + Default,
{
    pub fn new(
        prompt_registry: Arc<PromptRegistry>,
        model: Arc<P>,
        tools: Vec<Box<dyn SyncTool>>,
        memory: Arc<M>,
        max_iterations: usize,
    ) -> Self {
        Self {
            skeleton: CycleSkeleton::new(prompt_registry, model, tools, memory, max_iterations),
        }
    }

    pub fn run(&mut self, input: String) -> Result<String, AgentError> {
        let prompt = self.skeleton.build_prompt()?;

        // In production, this would be async. For now, synchronous step.
        let step_result = |skeleton: &mut CycleSkeleton<P, M>| -> Result<CycleAction, AgentError> {
            let tool_defs: Vec<ToolDefinition> = skeleton.tools.iter()
                .map(|t| ToolDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters(),
                }).collect();

            let response = skeleton.model.generate(&prompt, Some(tool_defs))?;
            Ok(parse_response(&response))
        };

        self.skeleton.run(input, step_result)
    }
}
```

> **Note:** The async/sync split here is a simplification. In the real implementation, `run()` will be `async fn` and call `model.generate()` with `.await`. The `CycleSkeleton::run()` will also be `async`.

- [ ] **Step 5: lib.rs**

```rust
// avs-react/src/lib.rs
pub mod cycle;
pub mod parse;
pub mod react;

pub use cycle::{CycleAction, CycleSkeleton};
pub use react::ReActStrategy;
```

- [ ] **Step 6: Tests**

```rust
// avs-react/tests/react_test.rs
use agentverse::{Config, Memory, Message, PromptRegistry, ShortTermMemory, ToolResult};
use agentverse_react::{CycleAction, ReActStrategy, parse::parse_response};
use serde_json::json;
use std::sync::Arc;

// Mock model for testing
struct TestModel {
    responses: Vec<String>,
    index: usize,
}

impl agentverse::model::ModelProvider for TestModel {
    async fn generate(&self, _prompt: &str, _tools: Option<Vec<agentverse::ToolDefinition>>) -> Result<String, agentverse::ModelError> {
        let resp = &self.responses[self.index % self.responses.len()];
        Ok(resp.clone())
    }
}

#[test]
fn test_parse_response_answer() {
    let result = parse_response("Thought: done.\nAnswer: Hello world");
    match result {
        CycleAction::Done { answer } => assert_eq!(answer, "Hello world"),
        _ => panic!("Expected Done"),
    }
}

#[test]
fn test_parse_response_tool_call() {
    let result = parse_response("Thought: searching.\nAction: search\nAction Input: {\"q\": \"test\"}");
    match result {
        CycleAction::ToolCall { tool_name, args } => {
            assert_eq!(tool_name, "search");
            assert_eq!(args["q"], "test");
        }
        _ => panic!("Expected ToolCall"),
    }
}
```

- [ ] **Step 7: Verify and commit**

Run: `cargo check -p agentverse-react`
Run: `cargo test -p agentverse-react`
Commit: `git add avs-react/ && git commit -m "feat: add ReAct orchestration strategy"`

---

## Task 2: avs-plan — Plan-and-Execute + Hierarchical Planning

**Files:**
- Create: `avs-plan/Cargo.toml`
- Create: `avs-plan/src/lib.rs`
- Create: `avs-plan/src/plan.rs`
- Create: `avs-plan/src/hierarchical.rs`
- Create: `avs-plan/src/planner.rs`
- Create: `avs-plan/tests/plan_test.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-plan"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
agentverse-react = { path = "../avs-react" }
serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
```

- [ ] **Step 2: planner.rs — Shared planning utilities**

```rust
// avs-plan/src/planner.rs
use agentverse::{ModelProvider, ToolDefinition};
use serde::{Deserialize, Serialize};

/// A single step in a plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: usize,
    pub description: String,
    pub tool: Option<String>,
    pub args: Option<serde_json::Value>,
    pub depends_on: Vec<usize>,  // IDs of steps this depends on
}

/// A complete plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub steps: Vec<PlanStep>,
    pub description: String,
}

impl Plan {
    pub fn is_complete(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Generate a plan from the LLM.
/// Prompt template for plan generation:
/// "Given the user request: {request}\nGenerate a step-by-step plan.\nRespond with JSON: {\"steps\": [{\"id\": 1, \"description\": \"...\", \"tool\": \"...\", \"args\": {...}}]}"
pub async fn generate_plan<P>(
    model: &P,
    request: &str,
    tools: &[String],
) -> Result<Plan, agentverse::ModelError>
where
    P: ModelProvider,
{
    let tools_desc = tools.join(", ");
    let prompt = format!(
        "You are a planning assistant. Given the following request and available tools, \
         generate a step-by-step plan.\n\nRequest: {}\nAvailable tools: {}\n\n\
         Respond with ONLY a JSON object with this structure:\n\
         {{\"description\": \"plan description\", \"steps\": [{{\"id\": 1, \"description\": \"step 1\", \"tool\": \"tool_name\", \"args\": {{}}}}]}}\n\nDo not include any text outside the JSON.",
        request, tools_desc
    );

    let response = model.generate(&prompt, None).await?;

    // Extract JSON from the response (handle markdown code blocks)
    let json_str = response
        .trim()
        .trim_start_matches(|c| c == '`')
        .trim_start_matches("json")
        .trim_start_matches(|c| c == '`')
        .trim();

    let plan: Plan = serde_json::from_str(json_str)
        .map_err(|e| agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse plan JSON: {}. Response was: {}", e, response
        )))?;

    Ok(plan)
}

/// Decompose a complex request into sub-goals (for Hierarchical Planning).
pub async fn decompose_request<P>(
    model: &P,
    request: &str,
) -> Result<Vec<String>, agentverse::ModelError>
where
    P: ModelProvider,
{
    let prompt = format!(
        "Given the following complex request, decompose it into sub-goals.\n\nRequest: {}\n\n\
         Respond with a JSON array of strings, one per sub-goal.\n\nDo not include any text outside the JSON.",
        request
    );

    let response = model.generate(&prompt, None).await?;
    let sub_goals: Vec<String> = serde_json::from_str(&response)
        .map_err(|e| agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse decomposition: {}", e
        )))?;

    Ok(sub_goals)
}
```

- [ ] **Step 3: plan.rs — Plan-and-Execute strategy**

```rust
// avs-plan/src/plan.rs
use super::planner::{generate_plan, Plan, PlanStep};
use agentverse::{AgentError, Memory, ModelProvider, PromptRegistry, SyncTool, ToolDefinition};
use std::sync::Arc;

/// Plan-and-Execute: generate a plan first, then execute steps sequentially.
pub struct PlanStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    model: Arc<P>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<M>,
    max_iterations: usize,
}

impl<P, M> PlanStrategy<P, M>
where
    P: ModelProvider + Clone,
    M: agentverse::Memory + Default,
{
    pub fn new(
        model: Arc<P>,
        tools: Vec<Box<dyn SyncTool>>,
        memory: Arc<M>,
        max_iterations: usize,
    ) -> Self {
        Self { model, tools, memory, max_iterations }
    }

    pub async fn run(&mut self, input: String) -> Result<String, AgentError> {
        // Phase 1: Generate plan
        let tool_names: Vec<String> = self.tools.iter().map(|t| t.name().to_string()).collect();
        let plan = generate_plan(&self.model, &input, &tool_names).await
            .map_err(|e| AgentError::Model(e))?;

        self.memory.append(agentverse::Message {
            role: agentverse::MessageRole::System,
            content: format!("Plan generated: {}", plan.description),
        });

        // Phase 2: Execute each step
        for step in &plan.steps {
            if let Some(tool_name) = &step.tool {
                let args = step.args.clone().unwrap_or_default();
                // Execute tool (reuse cycle skeleton's execute_tool logic)
                let result = self.execute_tool(tool_name, args)?;
                self.memory.append(agentverse::Message {
                    role: agentverse::MessageRole::Tool,
                    content: format!("Step {} executed: {}\nResult: {}", step.id, tool_name, result),
                });
            }
        }

        // Final answer
        let final_prompt = format!(
            "Plan was executed. Steps completed: {:?}\n\nBased on the results, provide the final answer to the user's request: {}\n\nConversation so far:\n{}",
            plan.steps.iter().map(|s| s.id).collect::<Vec<_>>(),
            input,
            self.memory.last_n(20).iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>().join("\n")
        );

        let answer = self.model.generate(&final_prompt, None)
            .await
            .map_err(|e| AgentError::Model(e))?;

        Ok(answer)
    }

    fn execute_tool(&self, tool_name: &str, args: serde_json::Value) -> Result<String, AgentError> {
        let tool = self.tools.iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string())))?;
        let result = tool.execute(args).map_err(|e| AgentError::Tool(e))?;
        Ok(result.to_string())
    }
}
```

- [ ] **Step 4: hierarchical.rs — Hierarchical Planning (single-agent, not multi-agent)**

```rust
// avs-plan/src/hierarchical.rs
use super::planner::{decompose_request, generate_plan, Plan};
use agentverse::{AgentError, Memory, ModelProvider, SyncTool};
use std::sync::Arc;

/// Hierarchical Planning: decompose → plan → execute.
/// NOT multi-agent: single agent handles all steps.
pub struct HierarchicalStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    model: Arc<P>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<M>,
    max_iterations: usize,
    max_decompose_depth: usize,
}

impl<P, M> HierarchicalStrategy<P, M>
where
    P: ModelProvider + Clone,
    M: agentverse::Memory + Default,
{
    pub fn new(
        model: Arc<P>,
        tools: Vec<Box<dyn SyncTool>>,
        memory: Arc<M>,
        max_iterations: usize,
        max_decompose_depth: usize,
    ) -> Self {
        Self { model, tools, memory, max_iterations, max_decompose_depth }
    }

    pub async fn run(&mut self, input: String) -> Result<String, AgentError> {
        // Phase 1: Decompose into sub-goals
        let sub_goals = decompose_request(&self.model, &input).await
            .map_err(|e| AgentError::Model(e))?;

        self.memory.append(agentverse::Message {
            role: agentverse::MessageRole::System,
            content: format!("Decomposed into {} sub-goals", sub_goals.len()),
        });

        // Phase 2: For each sub-goal, generate and execute a plan
        for (i, sub_goal) in sub_goals.iter().enumerate() {
            if i >= self.max_decompose_depth {
                break;
            }

            let tool_names: Vec<String> = self.tools.iter().map(|t| t.name().to_string()).collect();
            let sub_plan = generate_plan(&self.model, sub_goal, &tool_names).await
                .map_err(|e| AgentError::Model(e))?;

            // Execute sub-plan steps (reuse PlanStrategy logic)
            for step in &sub_plan.steps {
                if let Some(ref tool_name) = step.tool {
                    let args = step.args.clone().unwrap_or_default();
                    let result = self.execute_tool(tool_name, args)?;
                    self.memory.append(agentverse::Message {
                        role: agentverse::MessageRole::Tool,
                        content: format!("Sub-goal {} step {} ({}): {}", i, step.id, tool_name, result),
                    });
                }
            }
        }

        // Final answer synthesizing all sub-goal results
        let final_prompt = format!(
            "All sub-goals have been executed. Provide a comprehensive answer to: {}\n\nResult summary:\n{}",
            input,
            self.memory.last_n(30).iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>().join("\n")
        );

        let answer = self.model.generate(&final_prompt, None)
            .await
            .map_err(|e| AgentError::Model(e))?;

        Ok(answer)
    }

    fn execute_tool(&self, tool_name: &str, args: serde_json::Value) -> Result<String, AgentError> {
        let tool = self.tools.iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string())))?;
        let result = tool.execute(args).map_err(|e| AgentError::Tool(e))?;
        Ok(result.to_string())
    }
}
```

- [ ] **Step 5: lib.rs**

```rust
// avs-plan/src/lib.rs
pub mod hierarchical;
pub mod plan;
pub mod planner;

pub use hierarchical::HierarchicalStrategy;
pub use plan::PlanStrategy;
pub use planner::{Plan, PlanStep};
```

- [ ] **Step 6: Tests + verify + commit**

Run: `cargo check -p agentverse-plan`
Run: `cargo test -p agentverse-plan`
Commit: `git add avs-plan/ && git commit -m "feat: add Plan-and-Execute and Hierarchical Planning strategies"`

---

## Task 3: avs-router — Strategy Router

**Files:**
- Create: `avs-router/Cargo.toml`
- Create: `avs-router/src/lib.rs`
- Create: `avs-router/src/router.rs`
- Create: `avs-router/tests/router_test.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-router"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
```

- [ ] **Step 2: router.rs**

```rust
// avs-router/src/router.rs
use agentverse::{ModelProvider, ToolDefinition};
use serde::{Deserialize, Serialize};

/// Strategy names that the router can choose from
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

/// StrategyRouter: LLM-based dynamic routing.
/// At runtime, the router asks the LLM which strategy to use.
pub struct StrategyRouter<P>
where
    P: ModelProvider,
{
    model: P,
    strategies: Vec<StrategyName>,
}

impl<P> StrategyRouter<P>
where
    P: ModelProvider,
{
    pub fn new(model: P, strategies: Vec<StrategyName>) -> Self {
        Self { model, strategies }
    }

    /// Decide which strategy to use based on the user's request.
    pub async fn route(&self, request: &str) -> Result<StrategyName, agentverse::ModelError> {
        let strategy_list = self.strategies.iter()
            .map(|s| format!("{}: {}", s, strategy_description(s)))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Choose the best orchestration strategy for the following request.\n\nRequest: {}\n\n\
             Available strategies:\n{}\n\n\
             Respond with ONLY the strategy name (e.g., 'react', 'plan_and_execute', 'hierarchical').\n\
             Do not include any explanation.",
            request, strategy_list
        );

        let response = self.model.generate(&prompt, None).await?;
        let selected = response.trim().to_lowercase();

        match selected.as_str() {
            "react" => Ok(StrategyName::ReAct),
            "plan_and_execute" | "plan-and-execute" => Ok(StrategyName::PlanAndExecute),
            "hierarchical" => Ok(StrategyName::Hierarchical),
            _ => Err(agentverse::ModelError::InvalidResponse(format!(
                "Unknown strategy: {}", response
            ))),
        }
    }
}

fn strategy_description(strategy: &StrategyName) -> &'static str {
    match strategy {
        StrategyName::ReAct => "Best for: simple Q&A, tool use, step-by-step reasoning",
        StrategyName::PlanAndExecute => "Best for: tasks with clear steps that can be planned upfront",
        StrategyName::Hierarchical => "Best for: complex tasks that need decomposition into sub-goals",
    }
}
```

- [ ] **Step 3: lib.rs**

```rust
// avs-router/src/lib.rs
pub mod router;
pub use router::{StrategyName, StrategyRouter};
```

- [ ] **Step 4: Tests + verify + commit**

Run: `cargo check -p agentverse-router`
Run: `cargo test -p agentverse-router`
Commit: `git add avs-router/ && git commit -m "feat: add StrategyRouter for dynamic strategy selection"`

---

## Phase 2 Acceptance Criteria

- [ ] All 3 crates compile: `cargo check -p agentverse-react -p agentverse-plan -p agentverse-router`
- [ ] All tests pass: `cargo test -p agentverse-react -p agentverse-plan -p agentverse-router`
- [ ] Clippy passes: `cargo clippy -p agentverse-react -p agentverse-plan -p agentverse-router -- -D warnings`
- [ ] `ReActStrategy::run()` completes a full ReAct cycle (thought → action → answer)
- [ ] `PlanStrategy::run()` generates a plan and executes all steps
- [ ] `HierarchicalStrategy::run()` decomposes and executes sub-goals
- [ ] `StrategyRouter::route()` returns a valid `StrategyName`

## Parallel Execution Notes

- `avs-react` and `avs-plan` and `avs-router` are **independent** — they can be developed in parallel by 3 subagents
- All depend on `avs-core` being stable (Phase 1 complete)
- `avs-plan` depends on `agentverse-react` crate for shared cycle logic (reuse)

## Estimated Effort

~8-12 hours total. With 3 parallel subagents: ~4-6 hours (limited by avs-core stability and shared cycle design).
