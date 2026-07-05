use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SessionId = Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Interrupted,
    Completed,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::Active => write!(f, "active"),
            SessionStatus::Interrupted => write!(f, "interrupted"),
            SessionStatus::Completed => write!(f, "completed"),
        }
    }
}

impl std::str::FromStr for SessionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "interrupted" => Ok(Self::Interrupted),
            "completed" => Ok(Self::Completed),
            other => Err(format!("unknown status: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub user_id: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Session {
    pub fn new(user_id: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            user_id: user_id.to_string(),
            status: SessionStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }
}
