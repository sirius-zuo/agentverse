//! ReAct orchestration strategy for AgentVerse.
//!
//! Implements the ReAct pattern: Think → Act → Observe → Think...
//! Uses a shared cycle skeleton that all strategies can build on.

pub mod cycle;
pub mod parse;
pub mod react;

pub use agentverse::CycleResult;
pub use cycle::{CycleAction, CycleSkeleton};
pub use react::ReActStrategy;
