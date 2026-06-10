use serde::{Deserialize, Serialize};

pub type SkillId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: SkillId,
    pub version: String,
    pub description: String,
    pub tags: Vec<String>,
    /// Tool names declared by the skill (not yet intersected with registry).
    pub tools: Vec<String>,
    /// Stored but not used for routing in v1.
    pub activation_domains: Vec<String>,
    /// SKILL.md Markdown body.
    pub instructions: String,
    /// Contents of all supporting files in the skill directory.
    pub documents: Vec<String>,
    /// max_iterations hint — parsed but not applied to strategy in v1.
    pub max_iterations: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContext {
    pub instructions: String,
    pub documents: Vec<String>,
    /// Tool names for ActiveToolSet resolution.
    pub tools: Vec<String>,
    /// max_iterations hint — parsed but not applied in v1.
    pub max_iterations: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_context_round_trips_through_json() {
        let ctx = SkillContext {
            instructions: "You are an expert reviewer.".into(),
            documents: vec!["## Principles\nPrefer clarity.".into()],
            tools: vec!["FileSearch".into(), "WebSearch".into()],
            max_iterations: Some(10),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let restored: SkillContext = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.instructions, ctx.instructions);
        assert_eq!(restored.tools, ctx.tools);
        assert_eq!(restored.max_iterations, Some(10));
    }
}
