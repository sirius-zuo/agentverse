use agentverse::LlmRunnerBuilder;

#[test]
fn test_builder_requires_model() {
    let builder = LlmRunnerBuilder::new();
    let result = builder.build();
    assert!(result.is_err());
}
