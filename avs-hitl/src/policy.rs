use std::collections::{HashMap, HashSet};

pub type SkillId = String;

/// Immutable HITL policy built at agent startup.
/// Tier 1 (global_tool_blocklist) always wins; user skills cannot modify it.
/// Tier 2 (skill_tool_gates etc.) comes from system skills only.
#[derive(Debug, Default, Clone)]
pub struct HitlPolicy {
    /// Tier 1: always requires approval regardless of skill.
    pub global_tool_blocklist: HashSet<String>,
    /// Tier 2: per-skill tool gates (system skills only).
    pub skill_tool_gates: HashMap<SkillId, HashSet<String>>,
    /// Tier 2: skills requiring human approval before phase transition.
    pub skill_phase_gates: HashSet<SkillId>,
    /// Tier 2: named checkpoints per skill.
    pub skill_checkpoints: HashMap<SkillId, Vec<String>>,
}

impl HitlPolicy {
    pub fn new() -> Self {
        Self {
            global_tool_blocklist: [
                "file_delete",
                "exec_command",
                "system_shutdown",
                "database_delete",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            ..Default::default()
        }
    }

    pub fn from_system_skills<'a>(
        skills: impl IntoIterator<Item = &'a agentverse_skill::Skill>,
    ) -> Self {
        let mut policy = Self::new();

        for skill in skills {
            if !skill.hitl_tools.is_empty() {
                policy
                    .skill_tool_gates
                    .insert(skill.id.clone(), skill.hitl_tools.iter().cloned().collect());
            }
            if skill.phase_gate {
                policy.skill_phase_gates.insert(skill.id.clone());
            }
            if !skill.checkpoints.is_empty() {
                policy
                    .skill_checkpoints
                    .insert(skill.id.clone(), skill.checkpoints.clone());
            }
        }

        policy
    }

    pub fn requires_tool_approval(&self, skill_id: Option<&str>, tool_name: &str) -> bool {
        if self.global_tool_blocklist.contains(tool_name) {
            return true;
        }
        skill_id
            .and_then(|id| self.skill_tool_gates.get(id))
            .map(|gates| gates.contains(tool_name))
            .unwrap_or(false)
    }

    pub fn requires_phase_gate(&self, skill_id: &str) -> bool {
        self.skill_phase_gates.contains(skill_id)
    }

    pub fn is_checkpoint_tool(tool_name: &str) -> bool {
        tool_name == "request_checkpoint"
    }

    /// Tool names that must be called (and approved) before a skill
    /// invocation is allowed to finish: its gated `hitl_tools`, plus
    /// `request_checkpoint` if the skill declares any checkpoints.
    pub fn required_tool_names(&self, skill_id: Option<&str>) -> Vec<String> {
        let Some(id) = skill_id else {
            return Vec::new();
        };
        let mut names: Vec<String> = self
            .skill_tool_gates
            .get(id)
            .map(|gates| gates.iter().cloned().collect())
            .unwrap_or_default();
        if self
            .skill_checkpoints
            .get(id)
            .is_some_and(|cps| !cps.is_empty())
        {
            names.push("request_checkpoint".to_string());
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse_skill::Skill;

    fn skill(id: &str, hitl_tools: Vec<&str>, phase_gate: bool, checkpoints: Vec<&str>) -> Skill {
        Skill {
            id: id.into(),
            version: "1.0.0".into(),
            description: "test skill".into(),
            tags: vec![],
            tools: vec![],
            activation_domains: vec![],
            instructions: String::new(),
            documents: vec![],
            max_iterations: None,
            hitl_tools: hitl_tools.into_iter().map(str::to_owned).collect(),
            phase_gate,
            checkpoints: checkpoints.into_iter().map(str::to_owned).collect(),
        }
    }

    #[test]
    fn from_system_skills_includes_declared_hitl_metadata() {
        let system_skill = skill(
            "financial-review",
            vec!["ledger_post"],
            true,
            vec!["draft_ready", "before_send"],
        );

        let policy = HitlPolicy::from_system_skills([&system_skill]);

        assert!(policy.requires_tool_approval(Some("financial-review"), "ledger_post"));
        assert!(policy.requires_tool_approval(None, "exec_command"));
        assert!(policy.requires_phase_gate("financial-review"));
        assert_eq!(
            policy.skill_checkpoints.get("financial-review"),
            Some(&vec!["draft_ready".into(), "before_send".into()]),
        );
    }

    #[test]
    fn from_system_skills_ignores_parsed_metadata_outside_the_trusted_collection() {
        let system_skill = skill("financial-review", vec!["ledger_post"], false, vec![]);
        let user_skill = skill(
            "user-uploaded-skill",
            vec!["user_only_tool"],
            true,
            vec!["user_checkpoint"],
        );

        let policy = HitlPolicy::from_system_skills([&system_skill]);

        assert_eq!(user_skill.hitl_tools, vec!["user_only_tool"]);
        assert!(user_skill.phase_gate);
        assert_eq!(user_skill.checkpoints, vec!["user_checkpoint"]);
        assert!(!policy.requires_phase_gate(&user_skill.id));
        assert!(!policy.requires_tool_approval(Some(&user_skill.id), "user_only_tool"));
        assert!(!policy.skill_checkpoints.contains_key(&user_skill.id));
    }

    #[test]
    fn required_tool_names_combines_hitl_tools_and_checkpoint_marker() {
        let system_skill = skill(
            "financial-review",
            vec!["ledger_post"],
            false,
            vec!["draft_ready"],
        );
        let policy = HitlPolicy::from_system_skills([&system_skill]);

        let mut required = policy.required_tool_names(Some("financial-review"));
        required.sort();
        assert_eq!(
            required,
            vec!["ledger_post".to_string(), "request_checkpoint".to_string()]
        );
    }

    #[test]
    fn required_tool_names_empty_for_skill_with_no_hitl_metadata() {
        let system_skill = skill("plain-skill", vec![], false, vec![]);
        let policy = HitlPolicy::from_system_skills([&system_skill]);

        assert!(policy
            .required_tool_names(Some("plain-skill"))
            .is_empty());
        assert!(policy.required_tool_names(None).is_empty());
    }
}
