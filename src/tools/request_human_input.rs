//! Interactive human-input tool for HITL escalation (choice / text / secret / handoff).
//! 人机交互工具：需要密码、选项或交用户自助时调用，避免任务半途放弃。

use super::traits::{Tool, ToolExecutionContext, ToolResult};
use crate::approval::{HumanInputHub, HumanInputKind, HumanInputOutcome, HumanInputRequest};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

/// Tool that blocks until the operator answers via Web UI (or reports unavailable).
pub struct RequestHumanInputTool {
    hub: Arc<Mutex<Option<Arc<HumanInputHub>>>>,
}

impl RequestHumanInputTool {
    pub fn new() -> Self {
        Self {
            hub: Arc::new(Mutex::new(None)),
        }
    }

    pub fn hub_slot(&self) -> Arc<Mutex<Option<Arc<HumanInputHub>>>> {
        Arc::clone(&self.hub)
    }

    pub fn attach_hub(&self, hub: Arc<HumanInputHub>) {
        *self.hub.lock() = Some(hub);
    }
}

impl Default for RequestHumanInputTool {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_kind(raw: &str) -> Option<HumanInputKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "choice" => Some(HumanInputKind::Choice),
        "text" => Some(HumanInputKind::Text),
        "secret" => Some(HumanInputKind::Secret),
        "handoff" => Some(HumanInputKind::Handoff),
        _ => None,
    }
}

#[async_trait]
impl Tool for RequestHumanInputTool {
    fn name(&self) -> &str {
        "request_human_input"
    }

    fn description(&self) -> &str {
        "Ask the human operator for interactive help when a task cannot proceed alone \
         (sudo password, API token, choose among options, or hand off a command for them to run). \
         Prefer this over giving up. For secrets, the value never enters model context — you receive \
         a secret_slot id to pass to shell."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["choice", "text", "secret", "handoff"],
                    "description": "choice=pick option; text=non-secret string; secret=password/token (slot only); handoff=user runs something externally"
                },
                "prompt": {
                    "type": "string",
                    "description": "Clear instruction shown to the operator (include exact commands for handoff)"
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Required for kind=choice — short labels the operator can pick"
                },
                "risk_note": {
                    "type": "string",
                    "description": "Shown for secret/handoff — warn about password/token exposure risk"
                }
            },
            "required": ["kind", "prompt"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let kind_raw = args
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let Some(kind) = parse_kind(&kind_raw) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid kind. Use: choice | text | secret | handoff".into()),
            });
        };
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if prompt.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing non-empty 'prompt'".into()),
            });
        }
        let options: Vec<String> = args
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if kind == HumanInputKind::Choice && options.len() < 2 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("kind=choice requires options with at least 2 entries".into()),
            });
        }
        let risk_note = args
            .get("risk_note")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let hub = self.hub.lock().clone();
        let Some(hub) = hub else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "Interactive human input is not available on this channel. \
                     Tell the user the exact command to run themselves, then wait for their reply."
                        .into(),
                ),
            });
        };

        let outcome = hub
            .request(HumanInputRequest {
                kind,
                prompt,
                options,
                risk_note,
            })
            .await;

        let (success, output) = match outcome {
            HumanInputOutcome::Cancelled => (
                false,
                "Operator cancelled the interactive prompt. Offer an alternative path.".to_string(),
            ),
            HumanInputOutcome::TimedOut => (
                false,
                "Operator did not respond in time. Summarize what is blocked and ask again \
                 with a shorter handoff command."
                    .to_string(),
            ),
            HumanInputOutcome::Choice(v) => (
                true,
                format!("Operator selected: {v}\nContinue the task using this choice."),
            ),
            HumanInputOutcome::Text(v) => (
                true,
                format!("Operator provided text: {v}\nContinue the task using this value."),
            ),
            HumanInputOutcome::SecretSlot(slot) => (
                true,
                format!(
                    "Operator provided a secret. It is stored in secret_slot={slot} \
                     (value NOT included here and must NEVER be printed).\n\
                     To use with sudo, call shell with:\n\
                     - command: a `sudo -S ...` command (reads password from stdin)\n\
                     - secret_slot: {slot}\n\
                     The slot is one-shot and wiped after use."
                ),
            ),
            HumanInputOutcome::HandoffDone => (
                true,
                "Operator confirmed they completed the handoff action. \
                 Re-check the system state with a non-privileged command and continue."
                    .to_string(),
            ),
        };

        Ok(ToolResult {
            success,
            output,
            error: None,
        })
    }
}
