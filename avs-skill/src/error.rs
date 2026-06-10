#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("skill registry not configured: {0}")]
    NotConfigured(String),
    #[error("SKILL.md parse error in {path}: {message}")]
    Parse { path: String, message: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
