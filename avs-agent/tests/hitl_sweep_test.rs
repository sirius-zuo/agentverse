use agentverse_agent::HitlSweepConfig;
use agentverse_hitl::{
    ApprovalQueue, ApprovalRequest, ApprovalStatus, InMemoryQueue, InterruptKind,
};
use chrono::{Duration, Utc};
use std::sync::Arc;

#[tokio::test]
async fn sweep_expires_pending_request() {
    let queue = Arc::new(InMemoryQueue::new());
    let past = Utc::now() - Duration::seconds(1);
    let req = ApprovalRequest::new(
        uuid::Uuid::new_v4(),
        InterruptKind::SkillCheckpoint {
            checkpoint_name: "test".into(),
            payload: serde_json::json!({}),
        },
    )
    .with_expiry(past);
    let id = queue.submit(req).await.unwrap();

    // Before sweep: still pending
    assert_eq!(queue.poll(id).await.unwrap(), ApprovalStatus::Pending);

    // One sweep tick
    queue.sweep_expired().await.unwrap();

    // After sweep: expired
    assert_eq!(queue.poll(id).await.unwrap(), ApprovalStatus::Expired);

    // Verify HitlSweepConfig has defaults
    let _cfg = HitlSweepConfig::default();
}
