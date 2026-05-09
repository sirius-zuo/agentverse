use agentverse::{AgentError, ModelError, ToolError};

#[test]
fn test_error_display() {
    let err = ModelError::ApiError("401 Unauthorized".to_string());
    assert_eq!(err.to_string(), "API error: 401 Unauthorized");

    let err = ToolError::Execution("file not found".to_string());
    assert_eq!(err.to_string(), "Execution failed: file not found");
}

#[test]
fn test_agent_error_from_model() {
    let model_err = ModelError::Timeout("gpt-4".to_string());
    let agent_err = AgentError::Model(model_err);
    assert!(matches!(agent_err, AgentError::Model(_)));
}
