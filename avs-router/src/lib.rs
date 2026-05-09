//! Strategy router for AgentVerse.
//!
//! Provides LLM-based dynamic routing between orchestration strategies
//! (ReAct, Plan-and-Execute, Hierarchical).

pub mod router;

pub use router::{StrategyName, StrategyRouter};
