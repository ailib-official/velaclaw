use super::traits::{Tool, ToolExecutionContext, ToolResult};
use crate::runtime::RuntimeAdapter;
use crate::security::{NoopSandbox, PolicyHandle, ReceiptDecision, Sandbox, ToolReceiptLog};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Maximum shell command execution time before kill.
const SHELL_TIMEOUT_SECS: u64 = 60;
/// Maximum output size in bytes (1MB).
const MAX_OUTPUT_BYTES: usize = 1_048_576;
/// Environment variables safe to pass to shell commands.
/// Only functional variables are included — never API keys or secrets.
const SAFE_ENV_VARS: &[&str] = &[
    "PATH", "HOME", "TERM", "LANG", "LC_ALL", "LC_CTYPE", "USER", "SHELL", "TMPDIR",
];

/// Shell command execution tool with sandboxing
pub struct ShellTool {
    security: PolicyHandle,
    runtime: Arc<dyn RuntimeAdapter>,
    sandbox: Arc<dyn Sandbox>,
    receipts: Option<ToolReceiptLog>,
}

impl ShellTool {
    /// Test and default-tools constructor: Noop sandbox, no receipts.
    pub fn new(security: PolicyHandle, runtime: Arc<dyn RuntimeAdapter>) -> Self {
        Self {
            security,
            runtime,
            sandbox: Arc::new(NoopSandbox),
            receipts: None,
        }
    }

    /// Production constructor: OS sandbox + workspace receipts.
    pub fn with_isolation(
        security: PolicyHandle,
        runtime: Arc<dyn RuntimeAdapter>,
        sandbox: Arc<dyn Sandbox>,
        receipts: Option<ToolReceiptLog>,
    ) -> Self {
        Self {
            security,
            runtime,
            sandbox,
            receipts,
        }
    }

    fn record_receipt(&self, decision: ReceiptDecision, command: &str, human_approved: bool) {
        if let Some(log) = &self.receipts {
            if let Err(e) = log.record(
                "shell",
                decision,
                command,
                self.sandbox.name(),
                human_approved,
            ) {
                tracing::warn!("tool receipt write failed: {e}");
            }
        }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the workspace directory"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "secret_slot": {
                    "type": "string",
                    "description": "Opaque one-shot slot from request_human_input (kind=secret). \
                     Pipelines the secret to stdin (use `sudo -S ...`). Never put passwords in command."
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;
        let human_approved = ctx.human_shell_approved;

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        match self
            .security
            .validate_command_execution(command, human_approved)
        {
            Ok(_) => {}
            Err(reason) => {
                self.record_receipt(ReceiptDecision::Deny, command, human_approved);
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(reason),
                });
            }
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        // Execute with timeout to prevent hanging commands.
        // Clear the environment to prevent leaking API keys and other secrets
        // (CWE-200), then re-add only safe, functional variables.
        let mut cmd = match self
            .runtime
            .build_shell_command(command, &self.security.workspace_dir())
        {
            Ok(cmd) => cmd,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to build runtime command: {e}")),
                });
            }
        };
        cmd.env_clear();

        for var in SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        if let Err(e) = self.sandbox.wrap_command(cmd.as_std_mut()) {
            self.record_receipt(ReceiptDecision::SandboxFail, command, human_approved);
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Sandbox wrap failed: {e}")),
            });
        }

        self.record_receipt(ReceiptDecision::Allow, command, human_approved);

        let stdin_secret = ctx.stdin_secret.clone();
        let result = tokio::time::timeout(Duration::from_secs(SHELL_TIMEOUT_SECS), async {
            run_shell_command(cmd, stdin_secret).await
        })
        .await;

        match result {
            Ok(Ok(output)) => {
                let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();

                // Truncate output to prevent OOM
                if stdout.len() > MAX_OUTPUT_BYTES {
                    let n = crate::util::floor_char_boundary(&stdout, MAX_OUTPUT_BYTES);
                    stdout.truncate(n);
                    stdout.push_str("\n... [output truncated at 1MB]");
                }
                if stderr.len() > MAX_OUTPUT_BYTES {
                    let n = crate::util::floor_char_boundary(&stderr, MAX_OUTPUT_BYTES);
                    stderr.truncate(n);
                    stderr.push_str("\n... [stderr truncated at 1MB]");
                }

                Ok(ToolResult {
                    success: output.status.success(),
                    output: stdout,
                    error: if stderr.is_empty() {
                        None
                    } else {
                        Some(stderr)
                    },
                })
            }
            Ok(Err(e)) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute command: {e}")),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Command timed out after {SHELL_TIMEOUT_SECS}s and was killed"
                )),
            }),
        }
    }
}

async fn run_shell_command(
    mut cmd: tokio::process::Command,
    stdin_secret: Option<String>,
) -> std::io::Result<std::process::Output> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    // Turn Stop drops this future; kill the child of *this* turn (not an allowlist change).
    cmd.kill_on_drop(true);

    if stdin_secret.is_some() {
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            if let Some(secret) = stdin_secret {
                // `sudo -S` reads a password line from stdin.
                let _ = stdin.write_all(secret.as_bytes()).await;
                let _ = stdin.write_all(b"\n").await;
                let _ = stdin.shutdown().await;
            }
        }
        return child.wait_with_output().await;
    }

    cmd.output().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{NativeRuntime, RuntimeAdapter};
    use crate::security::{AutonomyLevel, PolicyHandle, SecurityPolicy};

    fn test_security(autonomy: AutonomyLevel) -> PolicyHandle {
        PolicyHandle::new(SecurityPolicy {
            autonomy,
            workspace_dir: std::env::temp_dir(),
            ..SecurityPolicy::default()
        })
    }

    fn test_runtime() -> Arc<dyn RuntimeAdapter> {
        Arc::new(NativeRuntime::new())
    }

    #[test]
    fn shell_tool_name() {
        let tool = ShellTool::new(test_security(AutonomyLevel::Supervised), test_runtime());
        assert_eq!(tool.name(), "shell");
    }

    #[test]
    fn shell_tool_description() {
        let tool = ShellTool::new(test_security(AutonomyLevel::Supervised), test_runtime());
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn shell_tool_schema_has_command() {
        let tool = ShellTool::new(test_security(AutonomyLevel::Supervised), test_runtime());
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["command"].is_object());
        assert!(schema["required"]
            .as_array()
            .expect("schema required field should be an array")
            .contains(&json!("command")));
        assert!(schema["properties"].get("approved").is_none());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_executes_allowed_command() {
        let tool = ShellTool::new(test_security(AutonomyLevel::Supervised), test_runtime());
        let result = tool
            .execute(
                json!({"command": "echo hello"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("echo command execution should succeed");
        assert!(result.success);
        assert!(result.output.trim().contains("hello"));
        assert!(result.error.is_none());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_blocks_disallowed_command() {
        let tool = ShellTool::new(test_security(AutonomyLevel::Supervised), test_runtime());
        let result = tool
            .execute(
                json!({"command": "rm -rf /"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("disallowed command execution should return a result");
        assert!(!result.success);
        let error = result.error.as_deref().unwrap_or("");
        assert!(error.contains("not allowed") || error.contains("high-risk"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_blocks_readonly() {
        let tool = ShellTool::new(test_security(AutonomyLevel::ReadOnly), test_runtime());
        let result = tool
            .execute(json!({"command": "ls"}), &ToolExecutionContext::default())
            .await
            .expect("readonly command execution should return a result");
        assert!(!result.success);
        assert!(result
            .error
            .as_ref()
            .expect("error field should be present for blocked command")
            .contains("read-only mode"));
    }

    #[tokio::test]
    async fn shell_missing_command_param() {
        let tool = ShellTool::new(test_security(AutonomyLevel::Supervised), test_runtime());
        let result = tool
            .execute(json!({}), &ToolExecutionContext::default())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("command"));
    }

    #[tokio::test]
    async fn shell_wrong_type_param() {
        let tool = ShellTool::new(test_security(AutonomyLevel::Supervised), test_runtime());
        let result = tool
            .execute(json!({"command": 123}), &ToolExecutionContext::default())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_captures_exit_code() {
        let tool = ShellTool::new(test_security(AutonomyLevel::Supervised), test_runtime());
        let result = tool
            .execute(
                json!({"command": "ls /nonexistent_dir_xyz"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("command with nonexistent path should return a result");
        assert!(!result.success);
    }

    fn test_security_with_env_cmd() -> PolicyHandle {
        PolicyHandle::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: std::env::temp_dir(),
            allowed_commands: vec!["env".into(), "echo".into()],
            ..SecurityPolicy::default()
        })
    }

    /// RAII guard that restores an environment variable to its original state on drop,
    /// ensuring cleanup even if the test panics.
    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(val) => std::env::set_var(self.key, val),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    #[cfg(unix)]
    async fn shell_does_not_leak_api_key() {
        let _g1 = EnvGuard::set("API_KEY", "sk-test-secret-12345");
        let _g2 = EnvGuard::set("VELACLAW_API_KEY", "sk-test-secret-67890");

        let tool = ShellTool::new(test_security_with_env_cmd(), test_runtime());
        let result = tool
            .execute(json!({"command": "env"}), &ToolExecutionContext::default())
            .await
            .expect("env command execution should succeed");
        assert!(result.success);
        assert!(
            !result.output.contains("sk-test-secret-12345"),
            "API_KEY leaked to shell command output"
        );
        assert!(
            !result.output.contains("sk-test-secret-67890"),
            "VELACLAW_API_KEY leaked to shell command output"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_preserves_path_and_home() {
        let tool = ShellTool::new(test_security_with_env_cmd(), test_runtime());

        let result = tool
            .execute(
                json!({"command": "echo $HOME"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("echo HOME command should succeed");
        assert!(result.success);
        assert!(
            !result.output.trim().is_empty(),
            "HOME should be available in shell"
        );

        let result = tool
            .execute(
                json!({"command": "echo $PATH"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("echo PATH command should succeed");
        assert!(result.success);
        assert!(
            !result.output.trim().is_empty(),
            "PATH should be available in shell"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_requires_approval_for_medium_risk_command() {
        let security = PolicyHandle::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            allowed_commands: vec!["touch".into()],
            workspace_dir: std::env::temp_dir(),
            ..SecurityPolicy::default()
        });

        let tool = ShellTool::new(security.clone(), test_runtime());
        let denied = tool
            .execute(
                json!({"command": "touch velaclaw_shell_approval_test"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("unapproved command should return a result");
        assert!(!denied.success);
        assert!(denied
            .error
            .as_deref()
            .unwrap_or("")
            .contains("requires explicit human approval"));

        let allowed = tool
            .execute(
                json!({"command": "touch velaclaw_shell_approval_test"}),
                &ToolExecutionContext::with_shell_human_approved(true),
            )
            .await
            .expect("approved command execution should succeed");
        assert!(allowed.success);

        let _ =
            tokio::fs::remove_file(std::env::temp_dir().join("velaclaw_shell_approval_test")).await;
    }

    // ── §5.2 Shell timeout enforcement tests ─────────────────

    #[test]
    fn shell_timeout_constant_is_reasonable() {
        assert_eq!(SHELL_TIMEOUT_SECS, 60, "shell timeout must be 60 seconds");
    }

    #[test]
    fn shell_output_limit_is_1mb() {
        assert_eq!(
            MAX_OUTPUT_BYTES, 1_048_576,
            "max output must be 1 MB to prevent OOM"
        );
    }

    // ── §5.3 Non-UTF8 binary output tests ────────────────────

    #[test]
    fn shell_safe_env_vars_excludes_secrets() {
        for var in SAFE_ENV_VARS {
            let lower = var.to_lowercase();
            assert!(
                !lower.contains("key") && !lower.contains("secret") && !lower.contains("token"),
                "SAFE_ENV_VARS must not include sensitive variable: {var}"
            );
        }
    }

    #[test]
    fn shell_safe_env_vars_includes_essentials() {
        assert!(
            SAFE_ENV_VARS.contains(&"PATH"),
            "PATH must be in safe env vars"
        );
        assert!(
            SAFE_ENV_VARS.contains(&"HOME"),
            "HOME must be in safe env vars"
        );
        assert!(
            SAFE_ENV_VARS.contains(&"TERM"),
            "TERM must be in safe env vars"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_blocks_rate_limited() {
        let security = PolicyHandle::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            max_actions_per_hour: 0,
            workspace_dir: std::env::temp_dir(),
            ..SecurityPolicy::default()
        });
        let tool = ShellTool::new(security, test_runtime());
        let result = tool
            .execute(
                json!({"command": "echo test"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("rate-limited command should return a result");
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or("").contains("Rate limit"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_pipes_stdin_secret_to_command() {
        let security = PolicyHandle::new(SecurityPolicy {
            autonomy: AutonomyLevel::Full,
            allowed_commands: vec!["cat".into()],
            workspace_dir: std::env::temp_dir(),
            ..SecurityPolicy::default()
        });
        let tool = ShellTool::new(security, test_runtime());
        let ctx = ToolExecutionContext::with_shell_human_approved(true)
            .with_stdin_secret(Some("slot-secret".into()));
        let result = tool
            .execute(json!({"command": "cat"}), &ctx)
            .await
            .expect("cat with stdin");
        assert!(result.success, "{:?}", result.error);
        assert!(result.output.contains("slot-secret"));
    }

    struct RecordingSandbox {
        wraps: std::sync::atomic::AtomicU32,
    }

    impl crate::security::Sandbox for RecordingSandbox {
        fn wrap_command(&self, _cmd: &mut std::process::Command) -> std::io::Result<()> {
            self.wraps.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        fn is_available(&self) -> bool {
            true
        }

        fn name(&self) -> &str {
            "recording"
        }

        fn description(&self) -> &str {
            "test double"
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn allowlisted_command_still_wraps_sandbox() {
        let recorder = Arc::new(RecordingSandbox {
            wraps: std::sync::atomic::AtomicU32::new(0),
        });
        let tool = ShellTool::with_isolation(
            test_security(AutonomyLevel::Supervised),
            test_runtime(),
            recorder.clone(),
            None,
        );
        let result = tool
            .execute(
                json!({"command": "echo hello"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("echo");
        assert!(result.success, "{:?}", result.error);
        assert_eq!(recorder.wraps.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn non_allowlisted_denied_when_approved_does_not_wrap() {
        let recorder = Arc::new(RecordingSandbox {
            wraps: std::sync::atomic::AtomicU32::new(0),
        });
        let security = PolicyHandle::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: std::env::temp_dir(),
            allowed_commands: vec!["echo".into()],
            ..SecurityPolicy::default()
        });
        let tool = ShellTool::with_isolation(security, test_runtime(), recorder.clone(), None);
        let result = tool
            .execute(
                json!({"command": "python3 -c 'print(1)'"}),
                &ToolExecutionContext::with_shell_human_approved(true),
            )
            .await
            .expect("deny");
        assert!(!result.success);
        assert_eq!(recorder.wraps.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fail_closed_sandbox_blocks_allowlisted_command() {
        let tool = ShellTool::with_isolation(
            test_security(AutonomyLevel::Supervised),
            test_runtime(),
            Arc::new(crate::security::FailClosedSandbox),
            None,
        );
        let result = tool
            .execute(
                json!({"command": "echo hello"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("result");
        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains("sandbox"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn receipts_record_allow_and_deny() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let receipts = crate::security::ToolReceiptLog::in_workspace(&workspace);
        let security = PolicyHandle::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: workspace.clone(),
            allowed_commands: vec!["echo".into()],
            ..SecurityPolicy::default()
        });
        let tool = ShellTool::with_isolation(
            security,
            test_runtime(),
            Arc::new(crate::security::NoopSandbox),
            Some(receipts.clone()),
        );
        let allowed = tool
            .execute(
                json!({"command": "echo hello"}),
                &ToolExecutionContext::default(),
            )
            .await
            .unwrap();
        assert!(allowed.success);
        let denied = tool
            .execute(
                json!({"command": "python3 -c 'print(1)'"}),
                &ToolExecutionContext::with_shell_human_approved(true),
            )
            .await
            .unwrap();
        assert!(!denied.success);
        let body = std::fs::read_to_string(receipts.path()).unwrap();
        assert!(body.contains("\"decision\":\"allow\""));
        assert!(body.contains("\"decision\":\"deny\""));
        assert!(!body.contains("slot-secret"));
    }
}
