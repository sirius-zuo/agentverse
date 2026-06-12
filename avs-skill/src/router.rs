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
        // Word-boundary check: skill id must appear as a whole whitespace-delimited token
        // (with trailing/leading punctuation stripped) to avoid false positives on substrings
        // (e.g. skill "hr" must not fire on "three" or "threshold").
        for skill in candidates {
            let skill_id = skill.id.to_lowercase();
            if msg_lower
                .split_whitespace()
                .any(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-') == skill_id)
            {
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
    // Split on non-alphanumeric boundaries (consistent with BM25Index tokenizer) so that
    // hyphenated IDs like "code-review" produce tokens ["code", "review"] in both message
    // and target rather than the single token "code-review".
    // Normalized by message word count: "what fraction of what the user said matches this skill."
    let msg_words: std::collections::HashSet<&str> = message
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if msg_words.is_empty() {
        return 0.0;
    }
    let target_words: std::collections::HashSet<&str> = target
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let matches = msg_words
        .iter()
        .filter(|w| target_words.contains(*w))
        .count();
    matches as f32 / msg_words.len() as f32
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
    fn explicit_name_match_wins_regardless_of_threshold() {
        let skills = vec![make_skill(
            "code-review",
            "Reviews code for bugs and style issues",
        )];
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
        let skills = vec![make_skill(
            "code-review",
            "Review code for bugs and style issues",
        )];
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
    fn keyword_overlap_scores_by_message_coverage() {
        // message: "review my code" (3 tokens); 2 of them appear in target → 2/3 ≈ 0.667
        let score = keyword_overlap("review my code", "review code bugs");
        assert!(score > 0.5 && score < 0.8, "expected ~0.667, got {}", score);
    }

    #[test]
    fn keyword_overlap_empty_target_returns_zero() {
        assert_eq!(keyword_overlap("hello world", ""), 0.0);
    }
}
