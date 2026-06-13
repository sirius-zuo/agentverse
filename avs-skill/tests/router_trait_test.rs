use agentverse_skill::{KeywordOverlapRouter, RouteSkills, Skill, SkillMode};

struct AlwaysNoneRouter;

impl RouteSkills for AlwaysNoneRouter {
    fn route(&self, _message: &str, _candidates: &[&Skill]) -> Option<String> {
        None
    }
}

#[test]
fn custom_router_implements_trait() {
    let router: Box<dyn RouteSkills> = Box::new(AlwaysNoneRouter);
    assert!(router.route("anything", &[]).is_none());
}

#[test]
fn keyword_overlap_router_renamed() {
    let router = KeywordOverlapRouter::for_mode(&SkillMode::Open);
    assert!(router.route("no overlap", &[]).is_none());
}
