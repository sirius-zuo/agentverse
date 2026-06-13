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
}
