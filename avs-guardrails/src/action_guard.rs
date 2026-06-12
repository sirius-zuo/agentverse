// avs-guardrails/src/action_guard.rs
use agentverse_hitl::{ApprovalId, ApprovalQueue, HitlPolicy, ApprovalRequest, InterruptKind};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

/// ActionGuard: pre-execution check for dangerous tool calls.
/// Returns Some(approval_id) when the call is intercepted, None when allowed.
pub struct ActionGuard {
    policy: Option<HitlPolicy>,
    queue:  Option<Arc<dyn ApprovalQueue>>,
}

impl ActionGuard {
    pub fn new() -> Self {
        Self { policy: None, queue: None }
    }

    pub fn with_policy(mut self, policy: HitlPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    pub fn with_queue(mut self, queue: Arc<dyn ApprovalQueue>) -> Self {
        self.queue = Some(queue);
        self
    }

    /// Returns Some(approval_id) when intercepted (caller must suspend + resume).
    /// Returns None when the tool is allowed to proceed immediately.
    pub async fn check(
        &self,
        tool_name:  &str,
        args:       &Value,
        session_id: Uuid,
    ) -> Option<ApprovalId> {
        let policy = self.policy.as_ref()?;
        let queue  = self.queue.as_ref()?;

        if !policy.requires_tool_approval(None, tool_name) {
            return None;
        }

        let kind = InterruptKind::ToolApproval {
            tool_name: tool_name.to_string(),
            args:      args.clone(),
        };
        let req = ApprovalRequest::new(session_id, kind);
        match queue.submit(req).await {
            Ok(id) => {
                tracing::warn!(tool = tool_name, approval_id = %id, "HITL: tool intercepted");
                Some(id)
            }
            Err(e) => {
                tracing::error!(error = %e, "HITL: queue submit failed; blocking tool as fail-safe");
                // Return a sentinel id — the tool is blocked. Resume will surface NotFound.
                Some(uuid::Uuid::new_v4())
            }
        }
    }
}

impl Default for ActionGuard {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_tool_returns_none() {
        let guard = ActionGuard::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(guard.check("file_read", &Value::Null, Uuid::new_v4()));
        assert!(result.is_none());
    }

    #[test]
    fn test_dangerous_tool_with_queue_is_intercepted() {
        use agentverse_hitl::InMemoryQueue;
        let queue = Arc::new(InMemoryQueue::new());
        let policy = HitlPolicy::new();
        let guard = ActionGuard::new()
            .with_policy(policy)
            .with_queue(Arc::clone(&queue) as Arc<dyn agentverse_hitl::ApprovalQueue>);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(guard.check("exec_command", &Value::Null, Uuid::new_v4()));
        assert!(result.is_some(), "exec_command must produce an approval request");
    }

    #[test]
    fn test_safe_tool_with_queue_returns_none() {
        use agentverse_hitl::InMemoryQueue;
        let queue = Arc::new(InMemoryQueue::new());
        let policy = HitlPolicy::new();
        let guard = ActionGuard::new()
            .with_policy(policy)
            .with_queue(Arc::clone(&queue) as Arc<dyn agentverse_hitl::ApprovalQueue>);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(guard.check("file_read", &Value::Null, Uuid::new_v4()));
        assert!(result.is_none(), "file_read should not be intercepted");
    }
}
