use agentverse::AgentBuilder;

#[test]
fn test_builder_requires_model() {
    let builder = AgentBuilder::new();
    let result = builder.build();
    assert!(result.is_err());
}
