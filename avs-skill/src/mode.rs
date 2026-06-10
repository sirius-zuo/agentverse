#[derive(Clone, Debug, Default, PartialEq)]
pub enum SkillMode {
    #[default]
    Open,
    Constrained(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_open() {
        assert_eq!(SkillMode::default(), SkillMode::Open);
    }

    #[test]
    fn constrained_holds_ids() {
        let mode = SkillMode::Constrained(vec!["hr-onboarding".into(), "hr-offboarding".into()]);
        match &mode {
            SkillMode::Constrained(ids) => {
                assert_eq!(ids.len(), 2);
                assert_eq!(ids[0], "hr-onboarding");
            }
            _ => panic!("expected Constrained"),
        }
    }
}
