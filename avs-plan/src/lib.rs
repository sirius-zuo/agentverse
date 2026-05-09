//! Plan-and-Execute and Hierarchical Planning strategies for AgentVerse.
//!
//! Provides two orchestration strategies:
//! - `PlanStrategy`: Generate a plan, then execute steps sequentially.
//! - `HierarchicalStrategy`: Decompose into sub-goals, plan each, then synthesize.

pub mod hierarchical;
pub mod plan;
pub mod planner;

pub use hierarchical::HierarchicalStrategy;
pub use plan::PlanStrategy;
pub use planner::{Plan, PlanStep};
