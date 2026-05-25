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

pub struct CleanupConfig {
    /// Delete raw turns older than this window.
    pub retention_window: Duration,
    /// How often the worker polls.
    pub poll_interval: Duration,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            retention_window: Duration::from_secs(86400), // 24h
            poll_interval: Duration::from_secs(300),      // 5 min
        }
    }
}

pub struct ConsolidationWorker {
    store: Arc<dyn SessionMemory>,
    memory_store: Arc<dyn LongtermMemory>,
    config: ConsolidationConfig,
}

impl ConsolidationWorker {
    pub fn new(
        store: Arc<dyn SessionMemory>,
        memory_store: Arc<dyn LongtermMemory>,
        config: ConsolidationConfig,
    ) -> Self {
        Self {
            store,
            memory_store,
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
        let sessions = self.store.list_all_active_sessions().await?;
        for session in sessions {
            let msgs = self.store.load_messages_above_watermark(session.id).await?;
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
                self.memory_store.write(&session.user_id, record).await?;
                // Advance watermark after each successful write to prevent duplicate writes on retry
                self.store.advance_watermark(session.id, *seq).await?;
            }
        }
        Ok(())
    }
}

pub struct CleanupWorker {
    store: Arc<dyn SessionMemory>,
    config: CleanupConfig,
}

impl CleanupWorker {
    pub fn new(store: Arc<dyn SessionMemory>, config: CleanupConfig) -> Self {
        Self { store, config }
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
        let cutoff_ts =
            chrono::Utc::now().timestamp() - self.config.retention_window.as_secs() as i64;
        let sessions = self.store.list_all_active_sessions().await?;
        for session in sessions {
            let wm = self.store.get_watermark(session.id).await?;
            let deleted = self
                .store
                .cleanup_expired_messages(session.id, cutoff_ts, wm)
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
        assert!(cfg.retention_window.as_secs() > 0);
        assert!(cfg.poll_interval.as_secs() > 0);
    }
}
