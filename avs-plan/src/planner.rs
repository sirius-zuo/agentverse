use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, LlmRunner, PromptRegistry};
use agentverse_guardrails::check_prompt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Deserialize a `Vec<usize>` where each element may be an integer or a
/// numeric string (models sometimes emit `"depends_on": ["1"]`).
fn deserialize_id_vec<'de, D>(deserializer: D) -> Result<Vec<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{SeqAccess, Visitor};

    struct IdVecVisitor;

    impl<'de> Visitor<'de> for IdVecVisitor {
        type Value = Vec<usize>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("array of integers or integer strings")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(v) = seq.next_element::<serde_json::Value>()? {
                let n = match v {
                    serde_json::Value::Number(n) => n
                        .as_u64()
                        .ok_or_else(|| serde::de::Error::custom("expected non-negative integer"))?
                        as usize,
                    serde_json::Value::String(s) => s.parse::<usize>().map_err(|_| {
                        serde::de::Error::custom(format!("cannot parse {s:?} as integer"))
                    })?,
                    other => {
                        return Err(serde::de::Error::custom(format!(
                            "expected integer or string, got {other}"
                        )))
                    }
                };
                out.push(n);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_seq(IdVecVisitor)
}

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
    #[serde(default, deserialize_with = "deserialize_id_vec")]
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
    runner: &LlmRunner,
    registry: &PromptRegistry,
    request: &str,
    tools: &str,
    conversation: &str,
) -> Result<Plan, AgentError> {
    let mut context = HashMap::new();
    context.insert(
        "tools".to_string(),
        serde_json::Value::String(tools.to_string()),
    );
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

    let messages = vec![
        Message {
            role: MessageRole::System,
            content: strategy_prompt,
        },
        Message {
            role: MessageRole::User,
            content: format!("Request: {}", request),
        },
    ];

    let response = runner.invoke(messages).await?;

    let raw = response.content.trim();
    let raw = raw.strip_prefix("```json").unwrap_or(raw);
    let raw = raw.strip_prefix("```").unwrap_or(raw);
    let raw = raw.strip_suffix("```").unwrap_or(raw);
    let raw = raw.trim();
    let json_str = match (raw.find('{'), raw.rfind('}')) {
        (Some(start), Some(end)) if end >= start => &raw[start..=end],
        _ => raw,
    };

    // Parse via Value first so duplicate keys (e.g. "args" appearing twice in
    // a step) are silently merged (last value wins) rather than hard-erroring.
    let value: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        AgentError::Model(agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse plan JSON: {}. Response was: {}",
            e, response.content
        )))
    })?;
    let plan: Plan = serde_json::from_value(value).map_err(|e| {
        AgentError::Model(agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse plan JSON: {}. Response was: {}",
            e, response.content
        )))
    })?;

    Ok(plan)
}

/// Decompose a complex request into sub-goals.
pub async fn decompose_request(
    runner: &LlmRunner,
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

    let messages = vec![
        Message {
            role: MessageRole::System,
            content: strategy_prompt,
        },
        Message {
            role: MessageRole::User,
            content: format!("Request: {}", request),
        },
    ];

    let response = runner.invoke(messages).await?;

    let raw = response.content.trim();
    let raw = raw.strip_prefix("```json").unwrap_or(raw);
    let raw = raw.strip_prefix("```").unwrap_or(raw);
    let raw = raw.strip_suffix("```").unwrap_or(raw);
    let raw = raw.trim();
    let json_str = match (raw.find('['), raw.rfind(']')) {
        (Some(start), Some(end)) if end >= start => &raw[start..=end],
        _ => raw,
    };

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
