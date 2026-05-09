//! Shared planning utilities for Plan-and-Execute and Hierarchical strategies.
//!
//! Provides `PlanStep`, `Plan`, and utility functions for generating plans
//! and decomposing requests into sub-goals.

use agentverse::ModelProvider;
use serde::{Deserialize, Serialize};

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

/// Generate a plan from the LLM given a user request and available tool names.
///
/// The LLM is prompted to return a JSON `Plan` object.
pub async fn generate_plan(
    model: &dyn ModelProvider,
    request: &str,
    tools: &[String],
) -> Result<Plan, agentverse::ModelError> {
    let tools_desc = if tools.is_empty() {
        "none (reasoning only)".to_string()
    } else {
        tools.join(", ")
    };

    let prompt = format!(
        "You are a planning assistant. Given the following request and available tools, \
         generate a step-by-step plan.\n\n\
         Request: {}\n\
         Available tools: {}\n\n\
         Respond with ONLY a JSON object with this structure:\n\
         {{\"description\": \"plan description\", \"steps\": [\n\
         {{\"id\": 1, \"description\": \"step 1\", \"tool\": \"tool_name\", \"args\": {{}}, \"depends_on\": []}},\n\
         {{\"id\": 2, \"description\": \"step 2\", \"tool\": null, \"args\": null, \"depends_on\": [1]}}\n\
         ]}}\n\n\
         Rules:\n\
         - Each step should be a single atomic action.\n\
         - Set \"tool\" to null and \"args\" to null for reasoning-only steps.\n\
         - Set \"depends_on\" to the IDs of steps that must complete first.\n\
         - Use the minimum number of steps necessary.\n\
         - Respond with ONLY the JSON object, no markdown, no explanation.",
        request, tools_desc
    );

    let response = model.generate(&prompt, None).await?;

    // Strip markdown code fences if present
    let json_str = response
        .trim()
        .trim_start_matches('`')
        .trim_start_matches("json")
        .trim_start_matches('`')
        .trim();

    let plan: Plan = serde_json::from_str(json_str).map_err(|e| {
        agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse plan JSON: {}. Response was: {}",
            e, response
        ))
    })?;

    Ok(plan)
}

/// Decompose a complex request into sub-goals using the LLM.
///
/// Returns a JSON array of strings, each representing one sub-goal.
pub async fn decompose_request(
    model: &dyn ModelProvider,
    request: &str,
) -> Result<Vec<String>, agentverse::ModelError> {
    let prompt = format!(
        "Given the following complex request, decompose it into sub-goals.\n\n\
         Request: {}\n\n\
         Respond with ONLY a JSON array of strings, one per sub-goal.\n\n\
         Do not include any text outside the JSON array.\n\n\
         Example response: [\"Sub-goal 1\", \"Sub-goal 2\", \"Sub-goal 3\"]",
        request
    );

    let response = model.generate(&prompt, None).await?;

    // Strip markdown code fences if present
    let json_str = response
        .trim()
        .trim_start_matches('`')
        .trim_start_matches("json")
        .trim_start_matches('`')
        .trim();

    let sub_goals: Vec<String> = serde_json::from_str(json_str).map_err(|e| {
        agentverse::ModelError::InvalidResponse(format!(
            "Failed to parse decomposition: {}. Response was: {}",
            e, response
        ))
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
