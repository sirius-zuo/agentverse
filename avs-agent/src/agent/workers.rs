use super::Agent;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// Wraps a worker factory in a restart-on-panic supervisor loop. Fixed 5s
/// backoff, unlimited retries — these are process-lifetime services;
/// "restart forever, don't hot-loop" is the right default.
fn spawn_supervised<F, Fut>(name: &'static str, make_worker: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match tokio::spawn(make_worker()).await {
                Ok(()) => {
                    tracing::error!(
                        worker = name,
                        "worker exited unexpectedly; restarting in 5s"
                    )
                }
                Err(e) => {
                    tracing::error!(worker = name, error = %e, "worker panicked; restarting in 5s")
                }
            }
            agentverse::metrics::record_worker_restart(name);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

impl Agent {
    /// Spawns the three background workers this agent needs, each supervised
    /// with restart-on-panic. Called once from `AgentBuilder::build`.
    pub(super) fn spawn_background_workers(
        self: &Arc<Self>,
        session_memory: Arc<dyn agentverse_session::SessionMemory>,
    ) {
        let sm_for_cleanup = Arc::clone(&session_memory);
        spawn_supervised("cleanup", move || {
            crate::workers::CleanupWorker::new(
                Arc::clone(&sm_for_cleanup),
                crate::workers::CleanupConfig::default(),
            )
            .run()
        });

        if let Some(ref hitl_cfg) = self.hitl {
            let queue = Arc::clone(&hitl_cfg.queue);
            spawn_supervised("hitl_sweep", move || {
                crate::workers::HitlSweepWorker::new(
                    Arc::clone(&queue),
                    crate::workers::HitlSweepConfig::default(),
                )
                .run()
            });
        }

        if let Some(ref ltm) = self.longterm_memory {
            let sm_for_consolidation = Arc::clone(&session_memory);
            let ltm = Arc::clone(ltm);
            spawn_supervised("consolidation", move || {
                crate::workers::ConsolidationWorker::new(
                    Arc::clone(&sm_for_consolidation),
                    Arc::clone(&ltm),
                    crate::workers::ConsolidationConfig::default(),
                )
                .run()
            });
        }
    }
}

#[cfg(test)]
mod supervision_tests {
    use super::spawn_supervised;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn supervisor_restarts_a_panicking_worker() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_worker = Arc::clone(&attempts);

        spawn_supervised("test-worker", move || {
            let attempts = Arc::clone(&attempts_for_worker);
            async move {
                let n = attempts.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    panic!("first attempt always panics");
                }
                // Second attempt: run forever so the test can observe the restart
                // happened without the supervisor looping indefinitely during the test.
                std::future::pending::<()>().await;
            }
        });

        // Supervisor's backoff is 5s in production; this test needs the real
        // panic-then-restart to happen well within a reasonable test timeout.
        // Poll for up to 8 seconds rather than sleeping a fixed amount.
        for _ in 0..80 {
            if attempts.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            attempts.load(Ordering::SeqCst) >= 2,
            "expected at least 2 attempts (initial + 1 restart) within 8s, got {}",
            attempts.load(Ordering::SeqCst)
        );
    }
}
