//! Per-channel inline approval hub (VL-SEC-003).
//! Telegram / messaging channels: prompt + wait for Y/N/Always or inline buttons.

use super::{ApprovalRequest, ApprovalResponse};
use crate::channels::traits::{Channel, SendMessage};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

const CALLBACK_PREFIX: &str = "vc_approve:";

/// Shared hub for resolving inline channel approvals.
#[derive(Clone, Default)]
pub struct ChannelApprovalHub {
    pending_by_chat: Arc<Mutex<HashMap<String, String>>>,
    pending_by_id: Arc<Mutex<HashMap<String, PendingEntry>>>,
}

struct PendingEntry {
    respond_tx: oneshot::Sender<ApprovalResponse>,
    chat_key: String,
}

impl ChannelApprovalHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn chat_key(channel: &str, reply_target: &str) -> String {
        format!("{channel}:{reply_target}")
    }

    fn inline_keyboard_markup(approval_id: &str) -> serde_json::Value {
        serde_json::json!({
            "inline_keyboard": [[
                {"text": "✅ Yes", "callback_data": format!("{CALLBACK_PREFIX}{approval_id}:yes")},
                {"text": "❌ No", "callback_data": format!("{CALLBACK_PREFIX}{approval_id}:no")},
                {"text": "♾ Always", "callback_data": format!("{CALLBACK_PREFIX}{approval_id}:always")}
            ]]
        })
    }

    /// Send an approval prompt and wait for user response or timeout.
    pub async fn request(
        &self,
        channel: Arc<dyn Channel>,
        channel_name: &str,
        reply_target: &str,
        request: &ApprovalRequest,
        summary: &str,
        timeout: Duration,
        shell_command: Option<&str>,
    ) -> ApprovalResponse {
        let id = Uuid::new_v4().to_string();
        let chat_key = Self::chat_key(channel_name, reply_target);
        let (tx, rx) = oneshot::channel();

        {
            self.pending_by_id.lock().insert(
                id.clone(),
                PendingEntry {
                    respond_tx: tx,
                    chat_key: chat_key.clone(),
                },
            );
            self.pending_by_chat.lock().insert(chat_key, id.clone());
        }

        let prompt = if let Some(command) = shell_command {
            format!(
                "🔒 Approve shell command?\n   {command}\n\nTap a button or reply Y / N / A (always)."
            )
        } else {
            format!(
                "🔧 Approve tool `{}`?\n   {summary}\n\nTap a button or reply Y / N / A (always).",
                request.tool_name
            )
        };

        let message = SendMessage::new(prompt, reply_target)
            .with_reply_markup(Self::inline_keyboard_markup(&id));

        if channel.send(&message).await.is_err() {
            self.remove_pending(&id);
            return ApprovalResponse::No;
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(decision)) => decision,
            _ => {
                self.remove_pending(&id);
                ApprovalResponse::No
            }
        }
    }

    fn remove_pending(&self, approval_id: &str) {
        let chat_key = self
            .pending_by_id
            .lock()
            .remove(approval_id)
            .map(|e| e.chat_key);
        if let Some(chat_key) = chat_key {
            self.pending_by_chat.lock().remove(&chat_key);
        }
    }

    fn resolve(&self, approval_id: &str, decision: ApprovalResponse) -> bool {
        let entry = self.pending_by_id.lock().remove(approval_id);
        let Some(entry) = entry else {
            return false;
        };
        self.pending_by_chat.lock().remove(&entry.chat_key);
        entry.respond_tx.send(decision).is_ok()
    }

    fn resolve_chat(&self, channel: &str, reply_target: &str, decision: ApprovalResponse) -> bool {
        let chat_key = Self::chat_key(channel, reply_target);
        let approval_id = self.pending_by_chat.lock().remove(&chat_key);
        let Some(approval_id) = approval_id else {
            return false;
        };
        let entry = self.pending_by_id.lock().remove(&approval_id);
        let Some(entry) = entry else {
            return false;
        };
        entry.respond_tx.send(decision).is_ok()
    }

    /// Parse Y/N/A text replies before normal message handling.
    pub fn try_resolve_text(&self, channel: &str, reply_target: &str, content: &str) -> bool {
        let decision = match content.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => Some(ApprovalResponse::Yes),
            "n" | "no" => Some(ApprovalResponse::No),
            "a" | "always" => Some(ApprovalResponse::Always),
            "!" | "never" => Some(ApprovalResponse::Never),
            _ => None,
        };
        let Some(decision) = decision else {
            return false;
        };
        self.resolve_chat(channel, reply_target, decision)
    }

    /// Resolve inline button callback (`vc_approve:<id>:yes|no|always`).
    pub fn try_resolve_callback(&self, callback_data: &str) -> bool {
        let Some(rest) = callback_data.strip_prefix(CALLBACK_PREFIX) else {
            return false;
        };
        let Some((id, action)) = rest.rsplit_once(':') else {
            return false;
        };
        let decision = match action {
            "yes" => ApprovalResponse::Yes,
            "no" => ApprovalResponse::No,
            "always" => ApprovalResponse::Always,
            "never" => ApprovalResponse::Never,
            _ => return false,
        };
        self.resolve(id, decision)
    }

    pub fn has_pending_for_chat(&self, channel: &str, reply_target: &str) -> bool {
        self.pending_by_chat
            .lock()
            .contains_key(&Self::chat_key(channel, reply_target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_reply_parses_yes_no_always() {
        let hub = ChannelApprovalHub::new();
        let (tx, mut rx) = oneshot::channel();
        hub.pending_by_id.lock().insert(
            "id-1".into(),
            PendingEntry {
                respond_tx: tx,
                chat_key: "telegram:123".into(),
            },
        );
        hub.pending_by_chat
            .lock()
            .insert("telegram:123".into(), "id-1".into());
        assert!(hub.try_resolve_text("telegram", "123", "yes"));
        assert_eq!(rx.try_recv().unwrap(), ApprovalResponse::Yes);
    }

    #[test]
    fn callback_data_parses_actions() {
        let hub = ChannelApprovalHub::new();
        let (tx, mut rx) = oneshot::channel();
        hub.pending_by_id.lock().insert(
            "test-id".into(),
            PendingEntry {
                respond_tx: tx,
                chat_key: "telegram:1".into(),
            },
        );
        hub.pending_by_chat
            .lock()
            .insert("telegram:1".into(), "test-id".into());
        assert!(hub.try_resolve_callback("vc_approve:test-id:no"));
        assert_eq!(rx.try_recv().unwrap(), ApprovalResponse::No);
    }
}
