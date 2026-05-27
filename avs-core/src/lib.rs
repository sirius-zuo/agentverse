//! AgentVerse: Lightweight, extensible AI Agent framework.
//!
//! ## Quick Start
//! ```
//! use agentverse::{LlmRunner, LlmRunnerBuilder};
//!
//! // Build a runner programmatically
//! let runner = LlmRunnerBuilder::new();
//! ```

pub mod builder;
pub mod config;
pub mod error;
pub mod example;
pub mod llm_runner;
pub mod memory;
pub mod model;
pub mod prompt;
pub mod strategy;
pub mod tool;
pub mod tracing;

// Public re-exports
pub use builder::LlmRunnerBuilder;
pub use config::{Config, ProviderConfig};
pub use error::{AgentError, ConfigError, GuardrailError, ModelError, ToolError};
pub use example::Example;
pub use llm_runner::LlmRunner;
pub use memory::{Memory, MemoryError, Message, MessageRole};
pub use model::{
    AnthropicProvider, ConnectionManager, CycleResult, GeminiProvider, GenerateRequest,
    GenerateResponse, ModelProvider, OpenAICompatible, ToolDefinition, UsageStats,
};
pub use prompt::PromptConfig;
pub use prompt::PromptRegistry;
pub use strategy::RunStrategy;
pub use tool::{ErasedTool, Tool, ToolCall, ToolCallResult, ToolHandle, ToolResult};
pub use tracing::{NoopTracer, Tracer};
