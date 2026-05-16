//! StrategyRouter: LLM-based dynamic routing between orchestration strategies.
//!
//! At runtime, the router asks the LLM which strategy to use for a given request.

use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, GenerateRequest, ModelProvider, PromptRegistry};
use agentverse_guardrails::check_prompt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Strategy names that the router can choose from.
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
///
/// At runtime, the router asks the LLM which strategy to use for a given request.
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
    /// Create a new StrategyRouter with the given model and available strategies.
    pub fn new(model: P, strategies: Vec<StrategyName>) -> Self {
        Self {
            model,
            strategies,
            registry: None,
        }
    }

    /// Create a router with prompt registry for templated prompts.
    pub fn with_registry(
        model: P,
        strategies: Vec<StrategyName>,
        registry: std::sync::Arc<PromptRegistry>,
    ) -> Self {
        Self {
            model,
            strategies,
            registry: Some(registry),
        }
    }

    /// Decide which strategy to use based on the user's request.
    ///
    /// Asks the LLM to choose the best strategy from the available options.
    pub async fn route(&self, request: &str) -> Result<StrategyName, AgentError> {
        let strategy_list = self
            .strategies
            .iter()
            .map(|s| format!("{}: {}", s, strategy_description(s)))
            .collect::<Vec<_>>()
            .join("\n");

        let system = if let Some(ref registry) = self.registry {
            let mut context = HashMap::new();
            context.insert(
                "conversation".to_string(),
                serde_json::Value::String(format!("User: {}", request)),
            );
            context.insert(
                "tools".to_string(),
                serde_json::Value::String(strategy_list),
            );

            let strategy_prompt = registry
                .render("router", context)
                .map_err(|e| AgentError::Config(agentverse::ConfigError::Invalid(e.to_string())))?;

            check_prompt(&strategy_prompt).map_err(|e| {
                AgentError::Guardrail(match e {
                    agentverse_guardrails::GuardrailError::PromptInjection(msg) => {
                        agentverse::GuardrailError::PromptInjection(msg)
                    }
                    agentverse_guardrails::GuardrailError::OutputFiltered(msg) => {
                        agentverse::GuardrailError::OutputFiltered(msg)
                    }
                    _ => agentverse::GuardrailError::PromptInjection(e.to_string()),
                })
            })?;

            format!(
                "{}\n\nRespond with ONLY the strategy name.",
                strategy_prompt
            )
        } else {
            // Fallback to hardcoded prompt if no registry
            format!(
                "Choose the best orchestration strategy for the following request.\n\n\
                 Available strategies:\n{}\n\n\
                 Respond with ONLY the strategy name (e.g., 'react', 'plan_and_execute', 'hierarchical').\n\
                 Do not include any explanation.",
                strategy_list
            )
        };

        let gen_request = GenerateRequest {
            system: Some(system),
            messages: vec![Message {
                role: MessageRole::User,
                content: format!("Request: {}", request),
            }],
            tools: None,
        };

        let response = self.model.generate(gen_request).await?;
        let selected = response.content.trim().to_lowercase();

        match selected.as_str() {
            "react" => Ok(StrategyName::ReAct),
            "plan_and_execute" | "plan-and-execute" => Ok(StrategyName::PlanAndExecute),
            "hierarchical" => Ok(StrategyName::Hierarchical),
            _ => Err(AgentError::Model(agentverse::ModelError::InvalidResponse(
                format!("Unknown strategy: {}", response.content),
            ))),
        }
    }

    /// Return the list of available strategies.
    pub fn available_strategies(&self) -> &[StrategyName] {
        &self.strategies
    }
}

/// Returns a human-readable description for a strategy name.
pub fn strategy_description(strategy: &StrategyName) -> &'static str {
    match strategy {
        StrategyName::ReAct => "Best for: simple Q&A, tool use, step-by-step reasoning",
        StrategyName::PlanAndExecute => {
            "Best for: tasks with clear steps that can be planned upfront"
        }
        StrategyName::Hierarchical => {
            "Best for: complex tasks that need decomposition into sub-goals"
        }
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
