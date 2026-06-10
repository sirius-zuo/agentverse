use crate::mode::SkillMode;
use crate::types::Skill;

pub struct SkillRouter {
    pub threshold: f32,
}

impl SkillRouter {
    pub fn for_mode(mode: &SkillMode) -> Self {
        let threshold = match mode {
            SkillMode::Open => 0.15,
            SkillMode::Constrained(_) => 0.08,
        };
        Self { threshold }
    }

    pub fn with_threshold(threshold: f32) -> Self {
        Self { threshold }
    }

    pub fn route(&self, message: &str, candidates: &[&Skill]) -> Option<String> {
        let msg_lower = message.to_lowercase();

        // Explicit name match always wins regardless of threshold.
        for skill in candidates {
            if msg_lower.contains(&skill.id.to_lowercase()) {
                return Some(skill.id.clone());
            }
        }

        // Keyword overlap scoring: pick the highest scorer above threshold.
        candidates
            .iter()
            .map(|skill| {
                let target = format!("{} {}", skill.id, skill.description).to_lowercase();
                let score = keyword_overlap(&msg_lower, &target);
                (score, skill.id.clone())
            })
            .filter(|(score, _)| *score >= self.threshold)
            .max_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, id)| id)
    }
}

pub(crate) fn keyword_overlap(message: &str, target: &str) -> f32 {
    let target_words: std::collections::HashSet<&str> = target.split_whitespace().collect();
    if target_words.is_empty() {
        return 0.0;
    }
    let matches = message
        .split_whitespace()
        .filter(|w| target_words.contains(w))
        .count();
    matches as f32 / target_words.len() as f32
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
        }
    }

    #[test]
    fn explicit_name_match_wins_regardless_of_threshold() {
        let skills = vec![make_skill("code-review", "Reviews code for bugs and style issues")];
        let router = SkillRouter::with_threshold(0.99); // impossibly high threshold
        let candidates: Vec<&Skill> = skills.iter().collect();
        assert_eq!(
            router.route("please use code-review on my file", &candidates),
            Some("code-review".into())
        );
    }

    #[test]
    fn keyword_match_above_threshold_activates() {
        let skills = vec![make_skill("code-review", "Review code for bugs and style")];
        let router = SkillRouter::with_threshold(0.1);
        let candidates: Vec<&Skill> = skills.iter().collect();
        assert_eq!(
            router.route("review my code for bugs", &candidates),
            Some("code-review".into())
        );
    }

    #[test]
    fn below_threshold_returns_none() {
        let skills = vec![make_skill("code-review", "Review code for bugs and style issues")];
        let router = SkillRouter::with_threshold(0.99);
        let candidates: Vec<&Skill> = skills.iter().collect();
        assert_eq!(router.route("what is the weather today", &candidates), None);
    }

    #[test]
    fn empty_candidates_returns_none() {
        let router = SkillRouter::for_mode(&SkillMode::Open);
        assert_eq!(router.route("anything at all", &[]), None);
    }

    #[test]
    fn picks_highest_scoring_candidate() {
        let skills = vec![
            make_skill("code-review", "Review code for bugs"),
            make_skill("docs-writer", "Write documentation markdown"),
        ];
        let router = SkillRouter::with_threshold(0.05);
        let candidates: Vec<&Skill> = skills.iter().collect();
        // "review code bugs" overlaps more with code-review description
        assert_eq!(
            router.route("review code bugs please", &candidates),
            Some("code-review".into())
        );
    }

    #[test]
    fn keyword_overlap_counts_shared_words_over_target_size() {
        // target: "review code bugs" (3 words)
        // message contains 2 of those words → 2/3 ≈ 0.667
        let score = keyword_overlap("review my code", "review code bugs");
        assert!(score > 0.5 && score < 0.8, "expected ~0.667, got {}", score);
    }

    #[test]
    fn keyword_overlap_empty_target_returns_zero() {
        assert_eq!(keyword_overlap("hello world", ""), 0.0);
    }
}
