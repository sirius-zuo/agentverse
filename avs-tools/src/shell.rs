use agentverse::{AsyncTool, ToolError, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Shell tool that runs commands with a fixed starting working directory.
///
/// Enforces: (1) commands start in `workdir`, (2) per-call timeout, (3) a
/// caller-supplied blocked-command list.
///
/// **Security note:** `workdir` is NOT a filesystem sandbox — absolute paths,
/// symlinks, and `cd` commands inside the shell can still access the full
/// filesystem. Pair with a blocked list and/or OS-level isolation (e.g.
/// containers, seccomp) for stronger security.
pub struct ShellTool {
    workdir: PathBuf,
    timeout_duration: Duration,
    blocked: Vec<String>,
}

impl ShellTool {
    /// Create a new ShellTool.
    ///
    /// - `workdir`: commands start here; this is NOT a filesystem sandbox —
    ///   absolute paths, symlinks, and `cd` can still reach the full filesystem.
    /// - `timeout_duration`: process is killed after this duration.
    /// - `blocked`: binary names the agent may not invoke (e.g. `["sudo", "rm"]`).
    ///   Pass an empty vec for no blocking.
    pub fn new(
        workdir: impl AsRef<Path>,
        timeout_duration: Duration,
        blocked: Vec<String>,
    ) -> Self {
        Self {
            workdir: workdir.as_ref().to_path_buf(),
            timeout_duration,
            blocked,
        }
    }
}

#[async_trait]
impl AsyncTool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the configured working directory."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to run (e.g. 'ls -la src/')"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let command = args["command"].as_str().ok_or_else(|| {
            ToolError::Execution("Missing 'command' parameter".to_string())
        })?;

        let parts = shell_words::split(command)
            .map_err(|e| ToolError::Execution(format!("Invalid command syntax: {e}")))?;

        if parts.is_empty() {
            return Err(ToolError::Execution("Command is empty".to_string()));
        }

        let binary = &parts[0];
        if self.blocked.iter().any(|b| b == binary) {
            return Err(ToolError::Execution(format!(
                "command '{binary}' is blocked"
            )));
        }

        let child = Command::new(binary)
            .args(&parts[1..])
            .current_dir(&self.workdir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ToolError::Execution(format!("Failed to spawn '{binary}': {e}")))?;

        let output = timeout(self.timeout_duration, child.wait_with_output())
            .await
            .map_err(|_| {
                ToolError::Execution(format!(
                    "command timed out after {}s",
                    self.timeout_duration.as_secs()
                ))
            })?
            .map_err(|e| ToolError::Execution(format!("Command execution failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code,
        }))
    }
}
