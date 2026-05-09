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
