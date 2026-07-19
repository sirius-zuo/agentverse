use crate::error::HitlError;
use crate::types::{ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalStatus};

#[async_trait::async_trait]
pub trait ApprovalQueue: Send + Sync {
    async fn submit(&self, req: ApprovalRequest) -> Result<ApprovalId, HitlError>;
    async fn resolve(&self, id: ApprovalId, decision: ApprovalDecision) -> Result<(), HitlError>;
    async fn poll(&self, id: ApprovalId) -> Result<ApprovalStatus, HitlError>;
    /// Mark expired approvals as rejected. Called by HitlSweepWorker.
    async fn sweep_expired(&self) -> Result<u64, HitlError>;
}
