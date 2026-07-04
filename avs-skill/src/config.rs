use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::error::SkillError;
use crate::mode::SkillMode;
use crate::registry::SkillRegistry;
use crate::types::Skill;

pub struct SkillConfig {
    pub registry: Arc<RwLock<SkillRegistry>>,
    pub mode: SkillMode,
    pub dir: PathBuf,
    /// Override routing threshold. None = use default per mode.
    pub routing_threshold: Option<f32>,
    /// Precomputed summaries block for the discovery phase; rebuilt on `reload_skills`.
    summaries: std::sync::Mutex<String>,
    /// Sorted eligible skill IDs; precomputed at load and rebuilt on `reload_skills`.
    pub ids: std::sync::Mutex<Vec<String>>,
}

impl SkillConfig {
    pub fn load(dir: impl AsRef<std::path::Path>, mode: SkillMode) -> Result<Self, SkillError> {
        let registry = SkillRegistry::load(dir.as_ref())?;
        let (summaries, ids) = {
            let candidates = registry.eligible(&mode);
            let s = format_skill_summaries(&candidates);
            let mut i: Vec<String> = candidates.iter().map(|s| s.id.clone()).collect();
            i.sort();
            (s, i)
        };
        Ok(Self {
            registry: Arc::new(RwLock::new(registry)),
            mode,
            dir: dir.as_ref().to_path_buf(),
            routing_threshold: None,
            summaries: std::sync::Mutex::new(summaries),
            ids: std::sync::Mutex::new(ids),
        })
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.routing_threshold = Some(threshold);
        self
    }

    /// Expose precomputed summaries string for agent prompt assembly.
    pub fn summaries(&self) -> String {
        self.summaries.lock().unwrap().clone()
    }

    /// Replace precomputed summaries (called by reload_skills in avs-agent).
    pub fn set_summaries(&self, text: String) {
        *self.summaries.lock().unwrap() = text;
    }

    /// Rebuild summaries and ids caches from a freshly loaded registry.
    /// Called by reload_skills after swapping the registry.
    pub fn rebuild_caches(&self, registry: &SkillRegistry) {
        let candidates = registry.eligible(&self.mode);
        let new_summaries = format_skill_summaries(&candidates);
        let mut new_ids: Vec<String> = candidates.iter().map(|s| s.id.clone()).collect();
        new_ids.sort();
        self.set_summaries(new_summaries);
        *self.ids.lock().unwrap() = new_ids;
    }
}

fn format_skill_summaries(skills: &[&Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut sorted = skills.to_vec();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let lines = sorted
        .iter()
        .map(|s| format!("- {}: {}", s.id, s.description))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "## Available Skills\n\n{}\n\nYou may use a skill when the user's request matches one of the above.",
        lines
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Skill;

    fn make_skill(id: &str, description: &str) -> Skill {
        Skill {
            id: id.into(),
            version: "1.0.0".into(),
            description: description.into(),
            tags: vec![],
            tools: vec![],
            activation_domains: vec![],
            instructions: String::new(),
            documents: vec![],
            max_iterations: None,
            hitl_tools: vec![],
            phase_gate: false,
            checkpoints: vec![],
        }
    }

    #[test]
    fn format_skill_summaries_is_sorted_and_contains_descriptions() {
        let skills = [
            make_skill("z-skill", "Does Z."),
            make_skill("a-skill", "Does A."),
        ];
        let refs: Vec<&Skill> = skills.iter().collect();
        let text = format_skill_summaries(&refs);
        assert!(text.contains("## Available Skills"));
        assert!(text.contains("a-skill: Does A."));
        assert!(text.contains("z-skill: Does Z."));
        // a-skill should appear before z-skill (sorted)
        assert!(text.find("a-skill").unwrap() < text.find("z-skill").unwrap());
    }

    #[test]
    fn format_skill_summaries_empty_list_returns_empty_string() {
        assert_eq!(format_skill_summaries(&[]), String::new());
    }
}
