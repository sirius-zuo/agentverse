//! AgentVerse: Lightweight, extensible AI Agent framework.
//!
//! ## Quick Start
//! ```
//! use agentverse::{Agent, AgentBuilder};
//!
//! // Build an agent programmatically
//! let agent = Agent::builder();
//! ```

pub mod agent;
pub mod builder;
pub mod config;
pub mod error;
pub mod example;
pub mod memory;
pub mod model;
pub mod prompt;
pub mod tool;
pub mod tracing;

// Public re-exports
pub use agent::Agent;
pub use builder::AgentBuilder;
pub use config::{Config, ProviderConfig};
pub use error::{AgentError, ConfigError, GuardrailError, ModelError, ToolError};
pub use example::Example;
pub use memory::{Memory, MemoryError, Message, MessageRole};
pub use model::{
    AnthropicProvider, CycleResult, GeminiProvider, GenerateRequest, GenerateResponse,
    ModelProvider, OpenAICompatible, ProviderWrapper, ToolDefinition, UsageStats,
};
pub use prompt::PromptConfig;
pub use prompt::PromptRegistry;
pub use tool::{AsyncTool, SyncTool, ToolResult};
pub use tracing::{NoopTracer, Tracer};
