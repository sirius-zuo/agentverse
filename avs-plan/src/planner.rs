use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, GenerateRequest, ModelProvider, PromptRegistry};
use agentverse_guardrails::check_prompt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single step in a plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStep {
    /// Unique step identifier.
    pub id: usize,
    /// Human-readable description of the step.
    pub description: String,
    /// Optional tool name to execute (if None, just reasoning).
    #[serde(default)]
    pub tool: Option<String>,
    /// Optional arguments for the tool.
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    /// IDs of steps this depends on (empty if no dependencies).
    #[serde(default)]
    pub depends_on: Vec<usize>,
}

/// A complete plan consisting of ordered steps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Plan {
    /// Description of the overall plan.
    #[serde(default)]
    pub description: String,
    /// Ordered list of steps to execute.
    pub steps: Vec<PlanStep>,
}

impl Plan {
    /// Returns `true` if the plan has no steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Generate a plan from the LLM using the templated prompt.
pub async fn generate_plan(
    model: &dyn ModelProvider,
    registry: &PromptRegistry,
    request: &str,
    tools: &[String],
    conversation: &str,
) -> Result<Plan, AgentError> {
    let tools_desc = if tools.is_empty() {
        "none (reasoning only)".to_string()
    } else {
        tools.join(", ")
    };

    let mut context = HashMap::new();
    context.insert("tools".to_string(), serde_json::Value::String(tools_desc));
    context.insert(
        "conversation".to_string(),
        serde_json::Value::String(conversation.to_string()),
    );
    if let Some(examples) = registry.get_examples("plan_examples") {
        if let Ok(val) = serde_json::to_value(examples) {
            context.insert("examples".to_string(), val);
        }
    }

    let strategy_prompt = registry
        .render("strategies.plan_and_execute", context)
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

    let request_obj = GenerateRequest {
        system: Some(format!(
            "{}\n\nRespond with ONLY a JSON object:\n{{\"description\": \"...\", \"steps\": [{{\"id\": 1, \"description\": \"...\", \"tool\": \"...\", \"args\": {{}}, \"depends_on\": []}}]}}",
            strategy_prompt
        )),
        messages: vec![Message {
            role: MessageRole::User,
            content: format!("Request: {}", request),
        }],
        tools: None,
    };

    let response = model.generate(request_obj).await?;

    let raw = response.content.trim();
    let raw = raw.strip_prefix("```json").unwrap_or(raw);
    let raw = raw.strip_prefix("```").unwrap_or(raw);
    let raw = raw.strip_suffix("```").unwrap_or(raw);
    let json_str = raw.trim();

    let plan: Plan = serde_json::from_str(json_str).map_err(|e| {
        AgentError::Model(agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse plan JSON: {}. Response was: {}",
            e, response.content
        )))
    })?;

    Ok(plan)
}

/// Decompose a complex request into sub-goals.
pub async fn decompose_request(
    model: &dyn ModelProvider,
    registry: &PromptRegistry,
    request: &str,
) -> Result<Vec<String>, AgentError> {
    let mut context = HashMap::new();
    context.insert(
        "conversation".to_string(),
        serde_json::Value::String(format!("User: {}", request)),
    );
    if let Some(examples) = registry.get_examples("hierarchical_examples") {
        if let Ok(val) = serde_json::to_value(examples) {
            context.insert("examples".to_string(), val);
        }
    }

    let strategy_prompt = registry
        .render("strategies.hierarchical.decompose", context)
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

    let request_obj = GenerateRequest {
        system: Some(format!(
            "{}\n\nRespond with ONLY a JSON array of strings.",
            strategy_prompt
        )),
        messages: vec![Message {
            role: MessageRole::User,
            content: format!("Request: {}", request),
        }],
        tools: None,
    };

    let response = model.generate(request_obj).await?;

    let raw = response.content.trim();
    let raw = raw.strip_prefix("```json").unwrap_or(raw);
    let raw = raw.strip_prefix("```").unwrap_or(raw);
    let raw = raw.strip_suffix("```").unwrap_or(raw);
    let json_str = raw.trim();

    let sub_goals: Vec<String> = serde_json::from_str(json_str).map_err(|e| {
        AgentError::Model(agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse decomposition: {}. Response was: {}",
            e, response.content
        )))
    })?;

    Ok(sub_goals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_is_empty() {
        let plan = Plan {
            description: "test".to_string(),
            steps: vec![],
        };
        assert!(plan.is_empty());

        let plan = Plan {
            description: "test".to_string(),
            steps: vec![PlanStep {
                id: 1,
                description: "step".to_string(),
                tool: None,
                args: None,
                depends_on: vec![],
            }],
        };
        assert!(!plan.is_empty());
    }

    #[test]
    fn test_plan_step_defaults() {
        let step = PlanStep {
            id: 1,
            description: "test".to_string(),
            tool: None,
            args: None,
            depends_on: vec![],
        };
        assert!(step.tool.is_none());
        assert!(step.args.is_none());
        assert!(step.depends_on.is_empty());
    }
}
