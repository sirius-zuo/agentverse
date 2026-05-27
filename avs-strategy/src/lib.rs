//! AgentVerse Strategy: Umbrella crate for strategy orchestration.
//!
//! Provides a unified interface for accessing all strategy implementations
//! and a factory function to create strategies at runtime.

pub use agentverse::RunStrategy;
pub use agentverse_plan::{HierarchicalStrategy, PlanStrategy};
pub use agentverse_react::ReActStrategy;

use agentverse::{LlmRunner, PromptRegistry};
use agentverse_tools::ToolRegistry;
use std::sync::Arc;

/// Enumeration of available strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyKind {
    React,
    Plan,
    Hierarchical,
}

/// Factory function to build a strategy given its kind and dependencies.
///
/// # Arguments
///
/// * `kind` - The strategy type to instantiate
/// * `runner` - Shared LLM runner for model invocations
/// * `prompts` - Prompt registry for template lookup
/// * `tools` - Tool registry for agent tool execution
/// * `max_iterations` - Maximum iterations for the strategy loop
///
/// # Returns
///
/// An `Arc<dyn RunStrategy>` that can be used to run the strategy.
///
/// # Example
///
/// ```ignore
/// let strategy = build(
///     StrategyKind::React,
///     runner,
///     prompts,
///     tools,
///     10,
/// );
/// let result = strategy.run(messages).await?;
/// ```
pub fn build(
    kind: StrategyKind,
    runner: Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools: Arc<ToolRegistry>,
    max_iterations: usize,
) -> Arc<dyn RunStrategy> {
    match kind {
        StrategyKind::React => Arc::new(ReActStrategy::new(runner, prompts, tools, max_iterations)),
        StrategyKind::Plan => Arc::new(PlanStrategy::new(runner, prompts, tools, max_iterations)),
        StrategyKind::Hierarchical => Arc::new(HierarchicalStrategy::new(
            runner,
            prompts,
            tools,
            max_iterations,
            3,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, ProviderConfig};
    use agentverse_tools::ToolRegistry;

    fn make_resources() -> (Arc<LlmRunner>, Arc<PromptRegistry>, Arc<ToolRegistry>) {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: ProviderConfig::OpenAI {
                    model_name: "test".to_string(),
                    api_key: "sk-test".to_string(),
                    base_url: Some("http://127.0.0.1:1/v1".to_string()),
                },
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        );
        (
            runner,
            Arc::new(PromptRegistry::new()),
            ToolRegistry::new(),
        )
    }

    #[test]
    fn build_react_strategy_returns_arc_dyn() {
        let (runner, prompts, tools) = make_resources();
        let _strategy = build(StrategyKind::React, runner, prompts, tools, 5);
    }

    #[test]
    fn build_plan_strategy_returns_arc_dyn() {
        let (runner, prompts, tools) = make_resources();
        let _strategy = build(StrategyKind::Plan, runner, prompts, tools, 5);
    }

    #[test]
    fn build_hierarchical_strategy_returns_arc_dyn() {
        let (runner, prompts, tools) = make_resources();
        let _strategy = build(StrategyKind::Hierarchical, runner, prompts, tools, 5);
    }
}
