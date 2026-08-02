use agentverse::PromptRegistry;
use agentverse_eval::runner::{load_toml_cases, RouterCase, TemplateCase};
use agentverse_skill::router::{KeywordOverlapRouter, RouteSkills};
use agentverse_skill::types::Skill;

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

fn toml_value_to_json(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(a) => {
            serde_json::Value::Array(a.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(t) => serde_json::Value::Object(
            t.into_iter()
                .map(|(k, v)| (k, toml_value_to_json(v)))
                .collect(),
        ),
    }
}

#[test]
fn template_fixture_cases_match_expected() {
    let cases: Vec<(String, TemplateCase)> = load_toml_cases("fixtures/templates");
    for (name, case) in cases {
        let mut registry = PromptRegistry::new();
        registry.add_template("under_test", &case.template).unwrap();
        let context: std::collections::HashMap<String, serde_json::Value> = case
            .context
            .into_iter()
            .map(|(k, v)| (k, toml_value_to_json(v)))
            .collect();
        let actual = registry.render("under_test", context).unwrap();
        assert_eq!(actual, case.expected, "fixture '{name}' mismatch");
    }
}
