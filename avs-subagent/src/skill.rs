use crate::result::SubAgentError;
use crate::spec::{Budget, ModelOverride, SubAgentSpec};
use serde::Deserialize;
use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize)]
struct SkillSubAgentYaml {
    name: String,
    objective: String,
    system_prompt: Option<String>,
    model: Option<ModelOverrideYaml>,
    #[serde(default)]
    allowed_tools: Vec<String>,
    budget: BudgetYaml,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ModelOverrideYaml {
    Plain(String),
    Tagged { r#type: String, value: String },
}

#[derive(Deserialize)]
struct BudgetYaml {
    max_steps: usize,
    max_tokens: u32,
    timeout: u64,
}

/// Load `subagent.yaml` from `skill_dir` if present. Returns `None` if the
/// file does not exist; returns `Err` if the file exists but cannot be parsed.
pub fn load_skill_subagent_spec(skill_dir: &Path) -> Result<Option<SubAgentSpec>, SubAgentError> {
    let yaml_path = skill_dir.join("subagent.yaml");
    let content = match std::fs::read_to_string(&yaml_path) {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(SubAgentError::Io(format!(
                "Failed to read {}: {e}",
                yaml_path.display()
            )))
        }
    };
    let raw: SkillSubAgentYaml = serde_yaml::from_str(&content).map_err(|e| {
        SubAgentError::Config(format!("Failed to parse {}: {e}", yaml_path.display()))
    })?;

    let model = raw.model.map(|m| match m {
        ModelOverrideYaml::Plain(s) => ModelOverride::Alias(s),
        ModelOverrideYaml::Tagged { r#type, value } => {
            if r#type == "id" {
                ModelOverride::Id(value)
            } else {
                ModelOverride::Alias(value)
            }
        }
    });

    Ok(Some(SubAgentSpec {
        name: raw.name,
        objective: raw.objective,
        system_prompt: raw.system_prompt,
        model,
        allowed_tools: raw.allowed_tools,
        budget: Budget {
            max_steps: raw.budget.max_steps,
            max_tokens: raw.budget.max_tokens,
            timeout: Duration::from_secs(raw.budget.timeout),
        },
    }))
}
