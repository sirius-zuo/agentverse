use agentverse::{Tool, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::timeout;

/// Shell tool that runs commands via `sh -c` with a fixed working directory.
///
/// **Security note:** `workdir` is NOT a filesystem sandbox — absolute paths,
/// symlinks, and `cd` commands can still access the full filesystem.
pub struct ShellTool {
    workdir: PathBuf,
    timeout_duration: Duration,
    blocked: Vec<String>,
}

impl ShellTool {
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

#[derive(Deserialize, JsonSchema)]
pub struct ShellArgs {
    /// Shell command to execute (supports pipes, redirections, variables)
    pub command: String,
}

#[async_trait::async_trait]
impl Tool for ShellTool {
    type Args = ShellArgs;

    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the configured working directory."
    }

    async fn execute(&self, args: ShellArgs) -> ToolResult {
        let parts = shell_words::split(&args.command)
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

        // Run via `sh -c` so pipes, redirections, and variables work.
        let child = tokio::process::Command::new("sh")
            .args(["-c", &args.command])
            .current_dir(&self.workdir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ToolError::Execution(format!("Failed to spawn command: {e}")))?;

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

        let mut out = stdout;
        if !stderr.trim().is_empty() {
            out.push_str(&format!("\n[stderr: {}]", stderr.trim()));
        }
        if exit_code != 0 {
            out.push_str(&format!("\n[exit code: {}]", exit_code));
        }
        Ok(json!(out))
    }
}
