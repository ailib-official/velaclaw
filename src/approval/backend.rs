//! App-layer approval adapters for runtime [`HumanApprovalBackend`] (VL-UR-002).
//! CLI / Gateway / Channel 批准后端：薄适配 runtime 契约。

use super::channel_hub::ChannelApprovalHub;
use super::{ApprovalHub, ApprovalManager, ApprovalRequest, ApprovalResponse};
use crate::channels::traits::Channel;
use crate::config::ChannelApprovalMode;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use velaclaw_agent_runtime::{shell_command_from_args, HumanApprovalBackend, ShellPolicyHook};

/// Per-message channel approval context (VL-SEC-003).
#[derive(Clone)]
pub struct ChannelApprovalSession {
    pub hub: Arc<ChannelApprovalHub>,
    pub channel: Arc<dyn Channel>,
    pub reply_target: String,
    pub sender: String,
    pub mode: ChannelApprovalMode,
    pub timeout: Duration,
}

/// Wraps [`ApprovalManager`] + optional [`ApprovalHub`] for one channel profile.
pub struct ManagerApprovalBackend<'a> {
    pub(crate) manager: &'a ApprovalManager,
    pub(crate) hub: Option<Arc<ApprovalHub>>,
    pub(crate) channel: &'a str,
    pub(crate) channel_session: Option<ChannelApprovalSession>,
}

impl<'a> ManagerApprovalBackend<'a> {
    pub fn new(manager: &'a ApprovalManager, channel: &'a str) -> Self {
        Self {
            manager,
            hub: None,
            channel,
            channel_session: None,
        }
    }

    pub fn with_hub(mut self, hub: Arc<ApprovalHub>) -> Self {
        self.hub = Some(hub);
        self
    }

    pub fn with_channel_session(mut self, session: ChannelApprovalSession) -> Self {
        self.channel_session = Some(session);
        self
    }

    async fn prompt_channel(
        &self,
        request: &ApprovalRequest,
        shell_command: Option<&str>,
    ) -> ApprovalResponse {
        let Some(session) = &self.channel_session else {
            return ApprovalResponse::No;
        };
        match session.mode {
            ChannelApprovalMode::Deny | ChannelApprovalMode::GatewayRedirect => {
                ApprovalResponse::No
            }
            ChannelApprovalMode::Inline => {
                let summary = summarize_args_for_backend(&request.arguments);
                session
                    .hub
                    .request(
                        Arc::clone(&session.channel),
                        self.channel,
                        &session.reply_target,
                        request,
                        &summary,
                        session.timeout,
                        shell_command,
                    )
                    .await
            }
        }
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
        let decision = if self.channel_session.is_some() {
            self.prompt_channel(&request, None).await
        } else if let Some(hub) = &self.hub {
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
        if self.channel == "cli" || self.hub.is_some() {
            return true;
        }
        self.channel_session
            .as_ref()
            .is_some_and(|s| s.mode == ChannelApprovalMode::Inline)
    }

    fn approve_shell_command_sync(&self, command: &str) -> bool {
        prompt_shell_security_approval(command)
    }

    async fn approve_shell_command_async(&self, command: &str) -> bool {
        if let Some(session) = &self.channel_session {
            if session.mode != ChannelApprovalMode::Inline {
                return false;
            }
            let request = ApprovalRequest {
                tool_name: "shell".into(),
                arguments: serde_json::json!({"command": command}),
            };
            let decision = self.prompt_channel(&request, Some(command)).await;
            self.manager
                .record_decision("shell", &request.arguments, decision, self.channel);
            return decision != ApprovalResponse::No;
        }
        self.approve_shell_command_sync(command)
    }
}

/// Backend that denies all interactive tool/shell approval (channel Deny profile).
pub struct DenyApprovalBackend;

#[async_trait]
impl HumanApprovalBackend for DenyApprovalBackend {
    fn needs_tool_approval(&self, _tool_name: &str) -> bool {
        true
    }

    fn approve_tool_sync(&self, _tool_name: &str, _arguments: &serde_json::Value) -> bool {
        false
    }

    async fn approve_tool_async(
        &self,
        _tool_name: &str,
        _arguments: &serde_json::Value,
    ) -> bool {
        false
    }

    fn interactive_shell_approval(&self) -> bool {
        false
    }

    fn approve_shell_command_sync(&self, _command: &str) -> bool {
        false
    }
}

/// [`PolicyHandle`] as runtime shell hook with live policy reads (VL-SEC-005).
pub struct PolicyHandleShellHook(pub crate::security::PolicyHandle);

impl ShellPolicyHook for PolicyHandleShellHook {
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
    use std::io::{self, BufRead, Write};
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

pub(crate) fn summarize_args_for_backend(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    let val = match v {
                        serde_json::Value::String(s) => truncate_for_summary(s, 80),
                        other => truncate_for_summary(&other.to_string(), 80),
                    };
                    format!("{k}: {val}")
                })
                .collect();
            parts.join(", ")
        }
        other => truncate_for_summary(&other.to_string(), 120),
    }
}

fn truncate_for_summary(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        input.to_string()
    }
}
