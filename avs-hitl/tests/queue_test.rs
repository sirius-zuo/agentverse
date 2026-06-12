use agentverse_hitl::{
    ApprovalDecision, ApprovalQueue, ApprovalRequest, ApprovalStatus, InMemoryQueue, InterruptKind,
};
use std::sync::Arc;
use uuid::Uuid;

fn make_request(session_id: Uuid) -> ApprovalRequest {
    ApprovalRequest::new(
        session_id,
        InterruptKind::ToolApproval {
            tool_name: "exec_command".to_string(),
            args: serde_json::json!({"cmd": "rm -rf /tmp/test"}),
        },
    )
}

#[tokio::test]
async fn submit_and_poll_pending() {
    let q = Arc::new(InMemoryQueue::new());
    let session_id = Uuid::new_v4();
    let req = make_request(session_id);
    let id = q.submit(req).await.unwrap();
    let status = q.poll(id).await.unwrap();
    assert_eq!(status, ApprovalStatus::Pending);
}

#[tokio::test]
async fn resolve_approved_then_poll_resolved() {
    let q = Arc::new(InMemoryQueue::new());
    let id = q.submit(make_request(Uuid::new_v4())).await.unwrap();
    q.resolve(id, ApprovalDecision::Approved).await.unwrap();
    let status = q.poll(id).await.unwrap();
    assert!(matches!(status, ApprovalStatus::Resolved(ApprovalDecision::Approved)));
}

#[tokio::test]
async fn resolve_rejected() {
    let q = Arc::new(InMemoryQueue::new());
    let id = q.submit(make_request(Uuid::new_v4())).await.unwrap();
    q.resolve(id, ApprovalDecision::Rejected { reason: "too risky".to_string() }).await.unwrap();
    let status = q.poll(id).await.unwrap();
    assert!(matches!(status, ApprovalStatus::Resolved(ApprovalDecision::Rejected { .. })));
}

#[tokio::test]
async fn poll_unknown_id_returns_not_found() {
    let q = InMemoryQueue::new();
    let result = q.poll(Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn sweep_expired_marks_expired_entries() {
    use chrono::Utc;
    let q = Arc::new(InMemoryQueue::new());
    let mut req = make_request(Uuid::new_v4());
    req.expires_at = Some(Utc::now() - chrono::Duration::seconds(1)); // already expired
    let id = q.submit(req).await.unwrap();
    let swept = q.sweep_expired().await.unwrap();
    assert_eq!(swept, 1);
    let status = q.poll(id).await.unwrap();
    assert_eq!(status, ApprovalStatus::Expired);
}
