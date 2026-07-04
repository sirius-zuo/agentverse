//! HITL (Human-in-the-Loop) protocol adapter for avs-core.
//!
//! `HitlHook` is the only cross-crate protocol adapter between avs-core
//! and avs-hitl — this is a necessary architectural trade-off:
//! `RunStrategy` must accept the hook without importing avs-hitl.

use serde_json::Value;
use uuid::Uuid;

pub type ApprovalId = Uuid;

#[async_trait::async_trait]
pub trait HitlHook: Send + Sync {
    /// Returns Some((approval_id, kind_json)) if the call is intercepted.
    /// Returns None if the tool is allowed to proceed.
    async fn check_tool(&self, tool_name: &str, args: &Value) -> Option<(ApprovalId, String)>;
}

pub struct HitlInterrupt {
    pub approval_id: ApprovalId,
    pub kind_json: String,
    pub history: Vec<crate::memory::Message>,
    pub pending_calls: Vec<crate::tool::ToolCall>,
    pub active_tool_names: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct AlwaysBlockHook;

    #[async_trait::async_trait]
    impl HitlHook for AlwaysBlockHook {
        async fn check_tool(&self, tool_name: &str, _args: &Value) -> Option<(ApprovalId, String)> {
            Some((Uuid::new_v4(), format!("{{\"tool\":\"{}\"}}", tool_name)))
        }
    }

    #[tokio::test]
    async fn hook_returns_approval_id_for_any_tool() {
        let hook: Arc<dyn HitlHook> = Arc::new(AlwaysBlockHook);
        let result = hook.check_tool("exec_command", &Value::Null).await;
        assert!(result.is_some());
    }

    struct NeverBlockHook;

    #[async_trait::async_trait]
    impl HitlHook for NeverBlockHook {
        async fn check_tool(&self, _: &str, _: &Value) -> Option<(ApprovalId, String)> {
            None
        }
    }

    #[tokio::test]
    async fn hook_returns_none_for_safe_tool() {
        let hook: Arc<dyn HitlHook> = Arc::new(NeverBlockHook);
        let result = hook.check_tool("file_read", &Value::Null).await;
        assert!(result.is_none());
    }
}
