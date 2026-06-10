#[derive(Debug, thiserror::Error)]
#[error("subagent error")]
pub struct SubAgentError;

#[allow(dead_code)]
pub type SubAgentResult<T> = Result<T, SubAgentError>;
