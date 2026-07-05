use agentverse_eval::runner::{load_toml_cases, ParserCase, RouterCase};
use agentverse_react::parse::parse_response;
use agentverse_skill::router::{KeywordOverlapRouter, RouteSkills};
use agentverse_skill::types::Skill;

#[test]
fn parser_fixture_cases_match_expected() {
    let cases: Vec<(String, ParserCase)> = load_toml_cases("fixtures/parser");
    for (name, case) in cases {
        let actual = parse_response(&case.input);
        let actual_debug = format!("{:?}", actual);
        assert_eq!(
            actual_debug, case.expected_debug,
            "fixture '{name}' mismatch: input={:?}",
            case.input
        );
    }
}

fn minimal_skill(id: &str) -> Skill {
    Skill {
        id: id.to_string(),
        version: "1.0.0".to_string(),
        description: format!("{id} skill"),
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
fn router_fixture_cases_match_expected() {
    let cases: Vec<(String, RouterCase)> = load_toml_cases("fixtures/router");
    for (name, case) in cases {
        let router = KeywordOverlapRouter::with_threshold(case.threshold);
        let skill = minimal_skill(&case.skill_id);
        let candidates = vec![&skill];
        let actual = router.route(&case.message, &candidates);
        let actual_debug = format!("{:?}", actual);
        assert_eq!(
            actual_debug, case.expected_debug,
            "fixture '{name}' mismatch: message={:?}",
            case.message
        );
    }
}
