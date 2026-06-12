#[derive(thiserror::Error, Debug)]
pub enum HitlError {
    #[error("approval not found: {0}")]
    NotFound(uuid::Uuid),
    #[error("database error: {0}")]
    Database(String),
    #[error("already resolved: {0}")]
    AlreadyResolved(uuid::Uuid),
}
