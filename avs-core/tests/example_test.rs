use agentverse::Example;
use serde::{Deserialize, Serialize};

#[test]
fn test_example_strategy_fields() {
    let ex = Example {
        input: "What time is it?".to_string(),
        output: None,
        strategy: Some("react".to_string()),
    };
    assert_eq!(ex.input, "What time is it?");
    assert!(ex.output.is_none());
    assert_eq!(ex.strategy, Some("react".to_string()));
}

#[test]
fn test_example_output_fields() {
    let ex = Example {
        input: "What is 2+2?".to_string(),
        output: Some("Thought: 2+2=4. Answer: 4".to_string()),
        strategy: None,
    };
    assert_eq!(ex.output, Some("Thought: 2+2=4. Answer: 4".to_string()));
    assert!(ex.strategy.is_none());
}

#[test]
fn test_example_roundtrip_json() {
    let ex = Example {
        input: "Hello".to_string(),
        output: Some("Hi there!".to_string()),
        strategy: None,
    };
    let json = serde_json::to_string(&ex).unwrap();
    let deserialized: Example = serde_json::from_str(&json).unwrap();
    assert_eq!(ex, deserialized);
}

#[test]
fn test_example_roundtrip_toml() {
    let ex = Example {
        input: "Hello".to_string(),
        output: Some("Hi there!".to_string()),
        strategy: None,
    };
    // TOML array of tables: [[example]]
    #[derive(Serialize, Deserialize)]
    struct ExampleSet {
        example: Vec<Example>,
    }
    let set = ExampleSet {
        example: vec![ex.clone()],
    };
    let toml_str = toml::to_string(&set).unwrap();
    let deserialized: ExampleSet = toml::from_str(&toml_str).unwrap();
    assert_eq!(deserialized.example, vec![ex]);
}
