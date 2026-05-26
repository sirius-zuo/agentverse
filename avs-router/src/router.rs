//! StrategyRouter: LLM-based dynamic routing between orchestration strategies.
//!
//! At runtime, the router asks the LLM which strategy to use for a given request.

use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, LlmRunner, PromptRegistry};
use agentverse_guardrails::check_prompt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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
pub struct StrategyRouter {
    runner: Arc<LlmRunner>,
    strategies: Vec<StrategyName>,
    registry: Option<Arc<PromptRegistry>>,
}

impl StrategyRouter {
    /// Create a new StrategyRouter with the given runner and available strategies.
    pub fn new(runner: Arc<LlmRunner>, strategies: Vec<StrategyName>) -> Self {
        Self {
            runner,
            strategies,
            registry: None,
        }
    }

    /// Create a router with prompt registry for templated prompts.
    pub fn with_registry(
        runner: Arc<LlmRunner>,
        strategies: Vec<StrategyName>,
        registry: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            runner,
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

        let messages = vec![
            Message {
                role: MessageRole::System,
                content: system,
            },
            Message {
                role: MessageRole::User,
                content: format!("Request: {}", request),
            },
        ];

        let response = self.runner.invoke(messages).await?;
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

#[cfg(test)]
mod new_tests {
    use super::*;
    use agentverse::{Config, LlmRunner};
    use std::sync::Arc;

    fn make_router() -> StrategyRouter {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::OpenAI {
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
        StrategyRouter::new(
            runner,
            vec![StrategyName::ReAct, StrategyName::PlanAndExecute],
        )
    }

    #[test]
    fn router_available_strategies_contains_react() {
        let router = make_router();
        assert!(router.available_strategies().contains(&StrategyName::ReAct));
    }

    #[tokio::test]
    async fn router_route_returns_error_on_bad_port() {
        let router = make_router();
        let result = router.route("search for rust").await;
        assert!(result.is_err());
    }
}
