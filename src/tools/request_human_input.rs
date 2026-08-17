//! Interactive human-input tool for HITL escalation (choice / short text / secret).
//! 人机交互工具：短选择、短明文、密钥；禁止把「去终端干活再交结果」当成 agent 流程。

use super::traits::{Tool, ToolExecutionContext, ToolResult};
use crate::approval::{HumanInputHub, HumanInputKind, HumanInputOutcome, HumanInputRequest};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

/// Max chars for `kind=text` — codes / short labels only, not command output dumps.
pub const MAX_SHORT_TEXT_CHARS: usize = 128;

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
        "Ask the human for a short interactive decision or credential when the task cannot \
         proceed alone. YOU remain the agent: run commands yourself via `shell` (the Web UI \
         will show Deny / Allow once / Always). Use this tool only for: \
         (1) kind=choice — Abort vs short options; \
         (2) kind=secret — sudo password / API token (returned as secret_slot, never in model context); \
         (3) kind=text — short codes only (pairing PIN, one-line id; not logs or command output). \
         Do NOT use this tool to make the human run terminal work and paste results back — \
         that is not an agent workflow. Prefer `shell` + approval. kind=handoff is legacy/rare \
         (off-machine physical steps only); never ask for pasted command output."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["choice", "text", "secret", "handoff"],
                    "description": "choice=short buttons; secret=password/token slot; text=short code only (≤128 chars); handoff=legacy rare off-machine confirm (avoid)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Clear short instruction for the modal (what is needed and why)"
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Required for kind=choice — short labels (include an Abort-style option when useful)"
                },
                "risk_note": {
                    "type": "string",
                    "description": "Optional risk line for secret (password stays on the local daemon)"
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
                     Do not ask the user to paste terminal output. Use `shell` where allowed, \
                     or explain what credential/config change is needed and wait for a short reply."
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
                "Operator cancelled the interactive prompt. Offer an alternative path \
                 (different approach via tools), or stop that step cleanly."
                    .to_string(),
            ),
            HumanInputOutcome::TimedOut => (
                false,
                "Operator did not respond in time. Summarize what is blocked; \
                 retry with a shorter choice/secret prompt, or continue via `shell` + approval."
                    .to_string(),
            ),
            HumanInputOutcome::Choice(v) => (
                true,
                format!("Operator selected: {v}\nContinue the task using this choice via tools."),
            ),
            HumanInputOutcome::Text(v) => {
                let trimmed = v.trim();
                if trimmed.chars().count() > MAX_SHORT_TEXT_CHARS {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Operator text exceeded {MAX_SHORT_TEXT_CHARS} characters. \
                             kind=text is for short codes only — do not collect command output \
                             via this tool. Use `shell` yourself after approval."
                        )),
                    });
                }
                (
                    true,
                    format!(
                        "Operator provided short text: {trimmed}\nContinue the task using this value via tools."
                    ),
                )
            }
            HumanInputOutcome::SecretSlot(slot) => (
                true,
                format!(
                    "Operator provided a secret. It is stored in secret_slot={slot} \
                     (value NOT included here and must NEVER be printed).\n\
                     To use with sudo, call shell with:\n\
                     - command: a `sudo -S ...` command (reads password from stdin)\n\
                     - secret_slot: {slot}\n\
                     The slot is one-shot and wiped after use. Do not ask the operator to run sudo themselves."
                ),
            ),
            HumanInputOutcome::HandoffDone => (
                true,
                "Operator confirmed a rare off-machine step. Re-check state with tools and continue. \
                 Do not ask them to paste command output; prefer `shell` + approval for machine work."
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kind_accepts_known_values() {
        assert_eq!(parse_kind("secret"), Some(HumanInputKind::Secret));
        assert_eq!(parse_kind("CHOICE"), Some(HumanInputKind::Choice));
        assert!(parse_kind("paste_results").is_none());
    }

    #[test]
    fn short_text_limit_is_tight() {
        const {
            assert!(MAX_SHORT_TEXT_CHARS <= 128);
        }
    }
}
