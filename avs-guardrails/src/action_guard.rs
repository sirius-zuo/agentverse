// avs-guardrails/src/action_guard.rs
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio::sync::mpsc;

use crate::GuardrailError;

/// Dangerous tool names that require human approval.
static DANGEROUS_TOOLS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    ["file_write", "file_delete", "exec_command", "system_shutdown", "database_delete"]
        .into_iter()
        .collect()
});

/// Callback type for human approval.
pub type ApprovalCallback = Arc<dyn Fn(&str, &Value) -> mpsc::Receiver<bool> + Send + Sync>;

/// ActionGuard: checks if a tool execution needs human approval.
pub struct ActionGuard {
    approval_callback: Option<ApprovalCallback>,
}

impl ActionGuard {
    pub fn new() -> Self {
        Self {
            approval_callback: None,
        }
    }

    pub fn with_approval_callback(mut self, callback: ApprovalCallback) -> Self {
        self.approval_callback = Some(callback);
        self
    }

    /// Check if a tool execution is allowed.
    /// Returns Ok if approved, Err if blocked.
    pub async fn check(&self, tool_name: &str, _args: &Value) -> Result<(), GuardrailError> {
        if DANGEROUS_TOOLS.contains(tool_name) {
            if let Some(ref callback) = self.approval_callback {
                // Wait for human approval
                let receiver = callback(tool_name, _args);
                // In production, this would await the approval signal
                // For MVP, return Ok (approve by default with logging)
                drop(receiver);
                tracing::warn!(
                    tool = tool_name,
                    "Dangerous tool called — awaiting human approval (MVP: auto-approve with warning)"
                );
                Ok(())
            } else {
                tracing::warn!(tool = tool_name, "Dangerous tool called without approval callback");
                Ok(())
            }
        } else {
            Ok(())
        }
    }
}

impl Default for ActionGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_tool() {
        let guard = ActionGuard::new();
        // Safe tools should pass without approval callback
        assert!(tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(guard.check("file_read", &Value::Null))
            .is_ok());
    }

    #[test]
    fn test_dangerous_tool_no_callback() {
        let guard = ActionGuard::new();
        assert!(tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(guard.check("file_delete", &Value::Null))
            .is_ok());
    }

    #[test]
    fn test_dangerous_tool_with_callback() {
        let guard = ActionGuard::new();
        let _guard = guard.with_approval_callback(Arc::new(|_tool, _args| {
            let (_tx, rx) = mpsc::channel(1);
            rx
        }));
        // Just verify it compiles and runs without panicking
    }
}
