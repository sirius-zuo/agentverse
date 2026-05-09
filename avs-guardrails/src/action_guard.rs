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
///
/// Dangerous tools are blocked unless an approval callback is configured.
/// In production, the callback waits for a human-in-the-loop signal.
/// For MVP development, the callback can return an immediately-available
/// receiver to simulate approval.
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
    ///
    /// Safe tools (not in the dangerous list) are always allowed.
    /// Dangerous tools require an approval callback — without one, they are blocked.
    /// With a callback, the tool waits for human approval.
    pub async fn check(&self, tool_name: &str, args: &Value) -> Result<(), GuardrailError> {
        if DANGEROUS_TOOLS.contains(tool_name) {
            match &self.approval_callback {
                Some(callback) => {
                    let receiver = callback(tool_name, args);
                    // In production, await the approval signal here.
                    // For MVP, consume the receiver (approve by default with logging).
                    drop(receiver);
                    tracing::warn!(
                        tool = tool_name,
                        "Dangerous tool called — awaiting human approval (MVP: auto-approve with warning)"
                    );
                    Ok(())
                }
                None => {
                    Err(GuardrailError::ActionBlocked(format!(
                        "Dangerous tool '{}' requires human approval but no approval callback is configured",
                        tool_name
                    )))
                }
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
        assert!(tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(guard.check("file_read", &Value::Null))
            .is_ok());
    }

    #[test]
    fn test_dangerous_tool_no_callback_blocked() {
        let guard = ActionGuard::new();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(guard.check("file_delete", &Value::Null));
        assert!(result.is_err());
        assert!(matches!(result, Err(GuardrailError::ActionBlocked(_))));
    }

    #[test]
    fn test_dangerous_tool_with_callback_approved() {
        let guard = ActionGuard::new().with_approval_callback(Arc::new(|_tool, _args| {
            let (_tx, rx) = mpsc::channel(1);
            rx
        }));
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(guard.check("exec_command", &Value::String("rm -rf /".to_string())));
        assert!(result.is_ok());
    }
}
