//! Unified approval gate — tool-level + shell policy human confirmation (VL-SEC-002).
//! 统一批准门：工具级交互 + shell 风险策略确认。

use super::{ApprovalHub, ApprovalManager, ApprovalRequest, ApprovalResponse};
use crate::agent::dispatcher::ParsedToolCall;
use crate::security::SecurityPolicy;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

/// Outcome of an approval gate check before tool execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Proceed { shell_human_approved: bool },
    Denied { message: String },
}

/// Channel-aware approval gate wrapping [`ApprovalManager`] and shell policy prompts.
pub struct ApprovalGate<'a> {
    manager: &'a ApprovalManager,
    hub: Option<Arc<ApprovalHub>>,
    channel: &'a str,
    security: Option<&'a SecurityPolicy>,
}

impl<'a> ApprovalGate<'a> {
    pub fn new(
        manager: &'a ApprovalManager,
        channel: &'a str,
        security: Option<&'a SecurityPolicy>,
    ) -> Self {
        Self {
            manager,
            hub: None,
            channel,
            security,
        }
    }

    pub fn with_hub(mut self, hub: Arc<ApprovalHub>) -> Self {
        self.hub = Some(hub);
        self
    }

    pub fn needs_approval(&self, tool_name: &str) -> bool {
        self.manager.needs_approval(tool_name)
    }

    /// Resolve human approval for one tool call (sync — CLI).
    pub fn decide_sync(&self, call: &ParsedToolCall) -> GateDecision {
        if let Some(denied) = self.tool_level_decision_sync(call) {
            return denied;
        }
        self.shell_policy_decision(&call.name, &call.arguments)
    }

    /// Resolve human approval for one tool call (async — gateway hub).
    pub async fn decide_async(&self, call: &ParsedToolCall) -> GateDecision {
        if let Some(denied) = self.tool_level_decision_async(call).await {
            return denied;
        }
        self.shell_policy_decision(&call.name, &call.arguments)
    }

    fn tool_level_decision_sync(&self, call: &ParsedToolCall) -> Option<GateDecision> {
        if !self.manager.needs_approval(&call.name) {
            return None;
        }
        let request = ApprovalRequest {
            tool_name: call.name.clone(),
            arguments: call.arguments.clone(),
        };
        let decision = if self.channel == "cli" {
            self.manager.prompt_cli(&request)
        } else {
            ApprovalResponse::No
        };
        self.manager
            .record_decision(&call.name, &call.arguments, decision, self.channel);
        if decision == ApprovalResponse::No {
            Some(GateDecision::Denied {
                message: "Denied by user.".into(),
            })
        } else {
            None
        }
    }

    async fn tool_level_decision_async(&self, call: &ParsedToolCall) -> Option<GateDecision> {
        if !self.manager.needs_approval(&call.name) {
            return None;
        }
        let request = ApprovalRequest {
            tool_name: call.name.clone(),
            arguments: call.arguments.clone(),
        };
        let decision = if let Some(hub) = &self.hub {
            self.manager.prompt_gateway(hub, &request).await
        } else if self.channel == "cli" {
            self.manager.prompt_cli(&request)
        } else {
            ApprovalResponse::No
        };
        self.manager
            .record_decision(&call.name, &call.arguments, decision, self.channel);
        if decision == ApprovalResponse::No {
            Some(GateDecision::Denied {
                message: "Denied by user.".into(),
            })
        } else {
            None
        }
    }

    fn shell_policy_decision(&self, tool_name: &str, args: &serde_json::Value) -> GateDecision {
        if !is_shell_policy_tool(tool_name) {
            return GateDecision::Proceed {
                shell_human_approved: false,
            };
        }
        let Some(sec) = self.security else {
            return GateDecision::Proceed {
                shell_human_approved: false,
            };
        };
        let Some(command) = shell_command_from_args(tool_name, args) else {
            return GateDecision::Proceed {
                shell_human_approved: false,
            };
        };
        if sec.validate_command_execution(command, false).is_ok() {
            return GateDecision::Proceed {
                shell_human_approved: false,
            };
        }
        if self.channel == "cli" || self.hub.is_some() {
            if prompt_shell_security_approval(command) {
                GateDecision::Proceed {
                    shell_human_approved: true,
                }
            } else {
                GateDecision::Denied {
                    message: "Denied by user.".into(),
                }
            }
        } else {
            GateDecision::Denied {
                message: format!(
                    "Command requires explicit human approval: {command}. \
                     Interactive approval is not available on this channel."
                ),
            }
        }
    }
}

fn is_shell_policy_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "shell" | "cron_add" | "cron_update" | "cron_run" | "schedule"
    )
}

fn shell_command_from_args<'a>(tool_name: &str, args: &'a serde_json::Value) -> Option<&'a str> {
    match tool_name {
        "shell" | "cron_add" | "cron_update" | "cron_run" | "schedule" => {
            args.get("command").and_then(|v| v.as_str())
        }
        _ => None,
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
