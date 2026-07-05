use agentverse::memory::{LongtermMemory, LongtermRecord};
use agentverse_session::SessionMemory;
use std::sync::Arc;
use std::time::Duration;

pub struct ConsolidationConfig {
    /// Consolidate when this many unconsolidated turns accumulate.
    pub batch_size: usize,
    /// Consolidate after this much idle time even if batch_size not reached.
    pub idle_timeout: Duration,
    /// How often the worker polls.
    pub poll_interval: Duration,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            idle_timeout: Duration::from_secs(1800), // 30 min
            poll_interval: Duration::from_secs(60),
        }
    }
}

#[derive(Clone)]
pub struct CleanupConfig {
    /// Delete raw turns older than this window, once consolidated.
    pub message_retention: Duration,
    /// Delete a session (and all its messages, via cascade) this long after it ends.
    pub session_retention: Duration,
    /// How often the worker polls.
    pub poll_interval: Duration,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            message_retention: Duration::from_secs(86400), // 24h
            session_retention: Duration::from_secs(2_592_000), // 30 days
            poll_interval: Duration::from_secs(300),       // 5 min
        }
    }
}

pub struct ConsolidationWorker {
    session_memory: Arc<dyn SessionMemory>,
    longterm_memory: Arc<dyn LongtermMemory>,
    config: ConsolidationConfig,
}

impl ConsolidationWorker {
    pub fn new(
        session_memory: Arc<dyn SessionMemory>,
        longterm_memory: Arc<dyn LongtermMemory>,
        config: ConsolidationConfig,
    ) -> Self {
        Self {
            session_memory,
            longterm_memory,
            config,
        }
    }

    /// Run the worker loop. Call via `tokio::spawn(worker.run())`.
    /// Loop runs until the task is cancelled.
    pub async fn run(self) {
        let mut interval = tokio::time::interval(self.config.poll_interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.tick().await {
                tracing::warn!(error = %e, "ConsolidationWorker tick error");
            }
        }
    }

    async fn tick(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let sessions = self
            .session_memory
            .list_sessions_needing_maintenance()
            .await?;
        agentverse::metrics::record_maintenance_backlog(sessions.len() as u64);
        for session in sessions {
            let msgs = self
                .session_memory
                .load_messages_above_watermark(session.id)
                .await?;
            if msgs.is_empty() {
                continue;
            }
            if msgs.len() < self.config.batch_size {
                // Check idle timeout: session.updated_at is the last-modified time
                let idle_secs = chrono::Utc::now().timestamp() - session.updated_at.timestamp();
                if idle_secs < self.config.idle_timeout.as_secs() as i64 {
                    continue;
                }
            }
            for (seq, msg) in &msgs {
                // TODO: replace with LLM summarizer once wired in
                let record = LongtermRecord::now(msg.content.clone(), 0.5);
                self.longterm_memory.write(&session.user_id, record).await?;
                // Advance watermark after each successful write to prevent duplicate writes on retry
                self.session_memory
                    .advance_watermark(session.id, *seq)
                    .await?;
            }
        }
        Ok(())
    }
}

pub struct CleanupWorker {
    session_memory: Arc<dyn SessionMemory>,
    config: CleanupConfig,
}

impl CleanupWorker {
    pub fn new(session_memory: Arc<dyn SessionMemory>, config: CleanupConfig) -> Self {
        Self {
            session_memory,
            config,
        }
    }

    /// Run the worker loop. Call via `tokio::spawn(worker.run())`.
    pub async fn run(self) {
        let mut interval = tokio::time::interval(self.config.poll_interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.tick().await {
                tracing::warn!(error = %e, "CleanupWorker tick error");
            }
        }
    }

    async fn tick(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Delete whole ended-and-expired sessions FIRST — a session removed
        // here never needs its individual messages evaluated below.
        let session_cutoff_ts =
            chrono::Utc::now().timestamp() - self.config.session_retention.as_secs() as i64;
        let sessions_deleted = self
            .session_memory
            .delete_ended_sessions_before(session_cutoff_ts)
            .await?;
        if sessions_deleted > 0 {
            tracing::info!(
                sessions_deleted,
                "CleanupWorker deleted ended sessions past retention"
            );
            agentverse::metrics::record_session_deleted(
                agentverse::metrics::SessionDeleteReason::EndedTtl,
                sessions_deleted,
            );
        }

        let message_cutoff_ts =
            chrono::Utc::now().timestamp() - self.config.message_retention.as_secs() as i64;
        let sessions = self
            .session_memory
            .list_sessions_needing_maintenance()
            .await?;
        agentverse::metrics::record_maintenance_backlog(sessions.len() as u64);
        for session in sessions {
            let wm = self.session_memory.get_watermark(session.id).await?;
            let deleted = self
                .session_memory
                .cleanup_expired_messages(session.id, message_cutoff_ts, wm)
                .await?;
            if deleted > 0 {
                tracing::debug!(
                    session_id = %session.id,
                    deleted,
                    "CleanupWorker purged expired turns"
                );
            }
        }
        Ok(())
    }
}

pub struct HitlSweepConfig {
    /// How often to run sweep_expired().
    pub poll_interval: Duration,
}

impl Default for HitlSweepConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(60),
        }
    }
}

pub struct HitlSweepWorker {
    queue: Arc<dyn agentverse_hitl::ApprovalQueue>,
    config: HitlSweepConfig,
}

impl HitlSweepWorker {
    pub fn new(queue: Arc<dyn agentverse_hitl::ApprovalQueue>, config: HitlSweepConfig) -> Self {
        Self { queue, config }
    }

    /// Run the worker loop. Call via `tokio::spawn(worker.run())`.
    pub async fn run(self) {
        let mut interval = tokio::time::interval(self.config.poll_interval);
        loop {
            interval.tick().await;
            match self.queue.sweep_expired().await {
                Ok(n) if n > 0 => {
                    tracing::info!(expired = n, "HitlSweepWorker expired approvals")
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "HitlSweepWorker sweep_expired error"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consolidation_config_defaults_are_sensible() {
        let cfg = ConsolidationConfig::default();
        assert!(cfg.batch_size > 0);
        assert!(cfg.idle_timeout.as_secs() > 0);
        assert!(cfg.poll_interval.as_secs() > 0);
    }

    #[test]
    fn cleanup_config_defaults_are_sensible() {
        let cfg = CleanupConfig::default();
        assert!(cfg.message_retention.as_secs() > 0);
        assert!(cfg.session_retention.as_secs() > 0);
        assert!(cfg.session_retention > cfg.message_retention);
        assert!(cfg.poll_interval.as_secs() > 0);
    }

    #[tokio::test]
    async fn cleanup_worker_deletes_ended_sessions_past_retention() {
        use agentverse::memory::{Message, MessageRole};
        use agentverse_session::{SessionMemory, SessionStatus, SqliteSessionMemory};

        let store = std::sync::Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
        let session = store.create("alice").await.unwrap();
        store
            .append_turn(
                session.id,
                Message {
                    role: MessageRole::User,
                    content: "hi".into(),
                },
                Message {
                    role: MessageRole::Assistant,
                    content: "hello".into(),
                },
            )
            .await
            .unwrap();
        store
            .update_status(session.id, SessionStatus::Completed)
            .await
            .unwrap();

        // `updated_at` and `delete_ended_sessions_before`'s cutoff are both
        // second-precision, and the comparison is strict (`<`); without this,
        // a fast test run lands the update and the cutoff in the same second
        // and the session is never seen as past retention.
        tokio::time::sleep(Duration::from_millis(1100)).await;

        let worker = CleanupWorker::new(
            store.clone(),
            CleanupConfig {
                message_retention: Duration::from_secs(86400),
                session_retention: Duration::from_secs(0), // anything ended is immediately eligible
                poll_interval: Duration::from_secs(300),
            },
        );
        worker.tick().await.unwrap();

        assert!(
            store.get(session.id).await.unwrap().is_none(),
            "session must be deleted once past session_retention (0s here, so immediately)"
        );
    }
}
