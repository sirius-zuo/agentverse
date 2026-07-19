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
    // HITL fields; AgentBuilder enforces only the trusted system-slot snapshot.
    pub hitl_tools: Vec<String>,
    pub phase_gate: bool,
    pub checkpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContext {
    pub skill_id: String,
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
            skill_id: "test-skill".into(),
            instructions: "You are an expert reviewer.".into(),
            documents: vec!["## Principles\nPrefer clarity.".into()],
            tools: vec!["FileSearch".into(), "WebSearch".into()],
            max_iterations: Some(10),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let restored: SkillContext = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.skill_id, ctx.skill_id);
        assert_eq!(restored.instructions, ctx.instructions);
        assert_eq!(restored.tools, ctx.tools);
        assert_eq!(restored.max_iterations, Some(10));
    }

    #[test]
    fn skill_context_round_trips_skill_id() {
        let ctx = SkillContext {
            skill_id: "billing-review".into(),
            instructions: "You review invoices.".into(),
            documents: vec![],
            tools: vec![],
            max_iterations: None,
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let restored: SkillContext = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.skill_id, "billing-review");
    }
}
