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

#[test]
fn test_rate_limited_error() {
    let err = ModelError::RateLimited("429 Too Many Requests".to_string());
    assert_eq!(err.to_string(), "Rate limited: 429 Too Many Requests");
}

#[test]
fn test_circuit_open_error() {
    let err = ModelError::CircuitOpen("Circuit breaker is open, retry later".to_string());
    assert_eq!(
        err.to_string(),
        "Circuit breaker open: Circuit breaker is open, retry later"
    );
}

#[test]
fn test_agent_error_from_rate_limited() {
    let err = ModelError::RateLimited("rate limit".to_string());
    let agent_err = AgentError::Model(err);
    assert!(matches!(agent_err, AgentError::Model(_)));
}
