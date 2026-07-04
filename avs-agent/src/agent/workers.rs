use super::Agent;
use std::sync::Arc;

impl Agent {
    /// Spawns the three background workers this agent needs. Called once from
    /// `AgentBuilder::build`. Identical behavior to the previous inline spawns
    /// in `Agent::new` — no supervision yet (see the following task for that).
    pub(super) fn spawn_background_workers(
        self: &Arc<Self>,
        session_memory: Arc<dyn agentverse_session::SessionMemory>,
    ) {
        let sm_for_workers = session_memory;

        // Auto-spawn CleanupWorker — purges stale messages from consolidated sessions
        tokio::spawn(
            crate::workers::CleanupWorker::new(
                Arc::clone(&sm_for_workers),
                crate::workers::CleanupConfig::default(),
            )
            .run(),
        );

        // Spawn HitlSweepWorker when HITL is configured
        if let Some(ref hitl_cfg) = self.hitl {
            tokio::spawn(
                crate::workers::HitlSweepWorker::new(
                    Arc::clone(&hitl_cfg.queue),
                    crate::workers::HitlSweepConfig::default(),
                )
                .run(),
            );
        }

        // Spawn ConsolidationWorker when longterm memory is configured
        if let Some(ref ltm) = self.longterm_memory {
            tokio::spawn(
                crate::workers::ConsolidationWorker::new(
                    Arc::clone(&sm_for_workers),
                    Arc::clone(ltm),
                    crate::workers::ConsolidationConfig::default(),
                )
                .run(),
            );
        }
    }
}
