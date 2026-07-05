use agentverse_eval::runner::{load_toml_cases, ParserCase};
use agentverse_react::parse::parse_response;

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
