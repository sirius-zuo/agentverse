use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type ApprovalId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterruptKind {
    ToolApproval {
        tool_name: String,
        args: serde_json::Value,
    },
    PhaseGate {
        from_skill: String,
        to_skill: String,
        deliverable: String,
    },
    SkillCheckpoint {
        checkpoint_name: String,
        payload: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: ApprovalId,
    pub session_id: Uuid,
    pub kind: InterruptKind,
    pub expires_at: Option<DateTime<Utc>>,
}

impl ApprovalRequest {
    pub fn new(session_id: Uuid, kind: InterruptKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            kind,
            expires_at: None,
        }
    }

    pub fn with_expiry(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalDecision {
    Approved,
    Rejected { reason: String },
    Modified { new_args: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalStatus {
    Pending,
    Resolved(ApprovalDecision),
    Expired,
}
