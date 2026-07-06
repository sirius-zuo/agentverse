use super::types::{Session, SessionId, SessionStatus};
use agentverse::memory::Message;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum SessionMemoryError {
    #[error("database error: {0}")]
    Database(String),
    #[error("session not found: {0}")]
    NotFound(SessionId),
}

#[async_trait]
pub trait SessionMemory: Send + Sync {
    async fn create(&self, user_id: &str) -> Result<Session, SessionMemoryError>;
    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionMemoryError>;
    async fn update_status(
        &self,
        session_id: SessionId,
        status: SessionStatus,
    ) -> Result<(), SessionMemoryError>;
    async fn list_by_user(&self, user_id: &str) -> Result<Vec<Session>, SessionMemoryError>;
    async fn append_message(
        &self,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionMemoryError>;
    async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionMemoryError>;
    async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionMemoryError>;
    async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionMemoryError>;
    async fn advance_watermark(
        &self,
        session_id: SessionId,
        new_watermark: i64,
    ) -> Result<(), SessionMemoryError>;
    /// Returns (sequence_num, Message) tuples for all messages above the current watermark.
    async fn load_messages_above_watermark(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<(i64, Message)>, SessionMemoryError>;
    /// Deletes messages where `created_at < cutoff_ts AND sequence_num <= watermark`.
    async fn cleanup_expired_messages(
        &self,
        session_id: SessionId,
        cutoff_ts: i64,
        watermark: i64,
    ) -> Result<u64, SessionMemoryError>;
    /// Returns every session — regardless of status — that still has pending
    /// maintenance work: unconsolidated messages (`sequence_num` above the
    /// watermark) or messages eligible for age-based pruning once consolidated.
    /// Used by background workers only. A session that leaves `Active` status
    /// remains visible here until fully drained — this is what makes worker
    /// crashes/restarts/retries self-healing regardless of *why* or *how* a
    /// session left `Active` (normal completion, HITL interruption, etc.).
    async fn list_sessions_needing_maintenance(&self) -> Result<Vec<Session>, SessionMemoryError>;

    /// Deletes every session with `status != Active` whose `updated_at` is
    /// older than `cutoff_ts`. Cascades to delete each session's messages.
    /// Returns the number of sessions deleted.
    async fn delete_ended_sessions_before(&self, cutoff_ts: i64)
        -> Result<u64, SessionMemoryError>;

    /// Deletes a single session immediately and unconditionally, regardless
    /// of status or age. Cascades to delete its messages. Used by the
    /// per-user on-demand deletion path — never by a background worker.
    async fn delete_session(&self, session_id: SessionId) -> Result<(), SessionMemoryError>;

    /// Store serialised skill context for a session (None clears it).
    /// Default implementation is a no-op so existing impls compile unchanged.
    async fn set_skill_context(
        &self,
        _session_id: SessionId,
        _context_json: Option<&str>,
    ) -> Result<(), SessionMemoryError> {
        Ok(())
    }

    /// Retrieve the serialised skill context for a session.
    /// Default returns None.
    async fn get_skill_context(
        &self,
        _session_id: SessionId,
    ) -> Result<Option<String>, SessionMemoryError> {
        Ok(None)
    }

    /// Create a session with skill context in a single operation.
    /// The default implementation is a two-step create + set; backends should
    /// override this with a single atomic INSERT to prevent orphaned sessions.
    async fn create_with_skill_context(
        &self,
        user_id: &str,
        context_json: &str,
    ) -> Result<Session, SessionMemoryError> {
        let session = self.create(user_id).await?;
        self.set_skill_context(session.id, Some(context_json))
            .await?;
        Ok(session)
    }

    /// Store the phase opening context for a session (None clears it).
    /// Injected as the sole prior context on the first invoke after a skill transition.
    /// Default is a no-op so existing impls compile unchanged.
    async fn set_phase_opening_context(
        &self,
        _session_id: SessionId,
        _context: Option<&str>,
    ) -> Result<(), SessionMemoryError> {
        if _context.is_some() {
            tracing::warn!(
                "set_phase_opening_context called on a SessionMemory implementation that uses \
                the default no-op. Phase transition context will not be persisted. \
                Override this method in your SessionMemory implementation if you use advance_phase."
            );
        }
        Ok(())
    }

    /// Retrieve the phase opening context for a session, if any.
    /// Default returns None.
    async fn get_phase_opening_context(
        &self,
        _session_id: SessionId,
    ) -> Result<Option<String>, SessionMemoryError> {
        Ok(None)
    }

    /// Apply a skill phase transition atomically: update `skill_context_json` and
    /// `phase_opening_context` in a single operation.
    /// The default falls back to two sequential writes for backward compatibility
    /// with custom implementations that have not yet overridden this method.
    async fn apply_phase_transition(
        &self,
        session_id: SessionId,
        skill_ctx_json: &str,
        phase_ctx: &str,
    ) -> Result<(), SessionMemoryError> {
        self.set_skill_context(session_id, Some(skill_ctx_json))
            .await?;
        self.set_phase_opening_context(session_id, Some(phase_ctx))
            .await?;
        Ok(())
    }

    /// Store serialised interrupted state for a session (None clears it).
    async fn set_interrupted_state(
        &self,
        _session_id: SessionId,
        _state_json: Option<&str>,
    ) -> Result<(), SessionMemoryError> {
        Ok(())
    }

    /// Retrieve the serialised interrupted state for a session.
    async fn get_interrupted_state(
        &self,
        _session_id: SessionId,
    ) -> Result<Option<String>, SessionMemoryError> {
        Ok(None)
    }
}

/// Serialised state stored when a session is suspended awaiting HITL approval.
/// Stored as JSON in the `interrupted_state` column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterruptedState {
    PendingToolCall {
        approval_id: String,
        kind_json: String,
        history_json: String,
        pending_calls_json: String,
        active_tool_names: Vec<String>,
        skill_context_json: Option<String>,
    },
    PendingPhaseGate {
        approval_id: String,
        transition_json: String,
    },
    PendingCheckpoint {
        approval_id: String,
        kind_json: String,
        history_json: String,
        active_tool_names: Vec<String>,
        skill_context_json: Option<String>,
    },
}

impl InterruptedState {
    pub fn approval_id_str(&self) -> &str {
        match self {
            Self::PendingToolCall { approval_id, .. } => approval_id,
            Self::PendingPhaseGate { approval_id, .. } => approval_id,
            Self::PendingCheckpoint { approval_id, .. } => approval_id,
        }
    }
}
