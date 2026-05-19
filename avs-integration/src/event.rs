use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Normalized message exchanged between connectors and the agent.
///
/// `conversation_id` routes replies back to the correct thread/channel.
/// Platform-specific details go in `metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub conversation_id: String,
    pub user_id: String,
    pub text: String,
    pub metadata: HashMap<String, String>,
}
