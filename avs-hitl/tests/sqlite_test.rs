use agentverse_hitl::{
    ApprovalDecision, ApprovalRequest, ApprovalStatus, InterruptKind, SqliteQueue,
};
use agentverse_hitl::ApprovalQueue;
use std::sync::Arc;
use uuid::Uuid;

async fn make_queue() -> Arc<SqliteQueue> {
    Arc::new(SqliteQueue::new("sqlite::memory:").await.unwrap())
}

#[tokio::test]
async fn sqlite_submit_and_poll_pending() {
    let q = make_queue().await;
    let req = ApprovalRequest::new(
        Uuid::new_v4(),
        InterruptKind::ToolApproval {
            tool_name: "exec_command".to_string(),
            args: serde_json::json!({}),
        },
    );
    let id = q.submit(req).await.unwrap();
    assert_eq!(q.poll(id).await.unwrap(), ApprovalStatus::Pending);
}

#[tokio::test]
async fn sqlite_resolve_approved() {
    let q = make_queue().await;
    let id = q.submit(ApprovalRequest::new(
        Uuid::new_v4(),
        InterruptKind::ToolApproval { tool_name: "exec_command".into(), args: serde_json::json!({}) },
    )).await.unwrap();
    q.resolve(id, ApprovalDecision::Approved).await.unwrap();
    assert!(matches!(q.poll(id).await.unwrap(), ApprovalStatus::Resolved(ApprovalDecision::Approved)));
}

#[tokio::test]
async fn sqlite_sweep_expired_marks_expired() {
    use chrono::Utc;
    let q = make_queue().await;
    let mut req = ApprovalRequest::new(
        Uuid::new_v4(),
        InterruptKind::ToolApproval { tool_name: "exec_command".into(), args: serde_json::json!({}) },
    );
    req.expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
    let id = q.submit(req).await.unwrap();
    let swept = q.sweep_expired().await.unwrap();
    assert_eq!(swept, 1);
    assert_eq!(q.poll(id).await.unwrap(), ApprovalStatus::Expired);
}
