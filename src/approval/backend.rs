//! App-layer approval adapters for runtime [`HumanApprovalBackend`] (VL-UR-002).
//! CLI / Gateway 批准后端：薄适配 runtime 契约。

use super::{ApprovalHub, ApprovalManager, ApprovalRequest, ApprovalResponse};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use velaclaw_agent_runtime::{shell_command_from_args, HumanApprovalBackend, ShellPolicyHook};

/// Wraps [`ApprovalManager`] + optional [`ApprovalHub`] for one channel profile.
pub struct ManagerApprovalBackend<'a> {
    pub(crate) manager: &'a ApprovalManager,
    pub(crate) hub: Option<Arc<ApprovalHub>>,
    pub(crate) channel: &'a str,
}

impl<'a> ManagerApprovalBackend<'a> {
    pub fn new(manager: &'a ApprovalManager, channel: &'a str) -> Self {
        Self {
            manager,
            hub: None,
            channel,
        }
    }

    pub fn with_hub(mut self, hub: Arc<ApprovalHub>) -> Self {
        self.hub = Some(hub);
        self
    }
}

#[async_trait]
impl HumanApprovalBackend for ManagerApprovalBackend<'_> {
    fn needs_tool_approval(&self, tool_name: &str) -> bool {
        self.manager.needs_approval(tool_name)
    }

    fn approve_tool_sync(&self, tool_name: &str, arguments: &serde_json::Value) -> bool {
        let request = ApprovalRequest {
            tool_name: tool_name.to_string(),
            arguments: arguments.clone(),
        };
        let decision = if self.channel == "cli" {
            self.manager.prompt_cli(&request)
        } else {
            ApprovalResponse::No
        };
        self.manager
            .record_decision(tool_name, arguments, decision, self.channel);
        decision != ApprovalResponse::No
    }

    async fn approve_tool_async(&self, tool_name: &str, arguments: &serde_json::Value) -> bool {
        let request = ApprovalRequest {
            tool_name: tool_name.to_string(),
            arguments: arguments.clone(),
        };
        let decision = if let Some(hub) = &self.hub {
            self.manager.prompt_gateway(hub, &request).await
        } else if self.channel == "cli" {
            self.manager.prompt_cli(&request)
        } else {
            ApprovalResponse::No
        };
        self.manager
            .record_decision(tool_name, arguments, decision, self.channel);
        decision != ApprovalResponse::No
    }

    fn interactive_shell_approval(&self) -> bool {
        self.channel == "cli" || self.hub.is_some()
    }

    fn approve_shell_command_sync(&self, command: &str) -> bool {
        prompt_shell_security_approval(command)
    }
}

/// [`SecurityPolicy`] as runtime shell hook (policy stays in app per UR-002).
pub struct SecurityPolicyShellHook<'a>(pub &'a SecurityPolicy);

impl ShellPolicyHook for SecurityPolicyShellHook<'_> {
    fn validate_shell_command(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        human_approved: bool,
    ) -> Result<(), String> {
        let Some(command) = shell_command_from_args(tool_name, arguments) else {
            return Ok(());
        };
        self.0
            .validate_command_execution(command, human_approved)
            .map(|_| ())
    }
}

fn prompt_shell_security_approval(command: &str) -> bool {
    eprintln!();
    eprintln!("🔒 Security policy requires approval for shell command:");
    eprintln!("   {command}");
    eprint!("   Approve this command? [Y/n]: ");
    let _ = io::stderr().flush();

    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_err() {
        return false;
    }

    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes" | "")
}
