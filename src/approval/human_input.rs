//! Interactive human-input hub for Web UI HITL (choice / short text / secret / rare handoff).
//! Web 人机交互：短选项、短明文、密钥；handoff 仅保留罕见机外确认。

use super::secret_slots::SecretSlotStore;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;

const DEFAULT_INPUT_TIMEOUT: Duration = Duration::from_secs(600);

/// Kind of interactive prompt shown to the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanInputKind {
    /// Pick one of `options` (short labels).
    Choice,
    /// Free-form non-secret **short** text (codes / ids — not command dumps).
    Text,
    /// Secret (password / token / pairing code) — stored in a local slot only.
    Secret,
    /// Rare off-machine confirmation; not for “run this in your terminal”.
    Handoff,
}

/// Request payload for [`HumanInputHub::request`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanInputRequest {
    pub kind: HumanInputKind,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub risk_note: Option<String>,
}

/// Event pushed to WebSocket clients.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HumanInputRequiredEvent {
    pub id: String,
    pub kind: HumanInputKind,
    pub prompt: String,
    pub options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_note: Option<String>,
}

/// Operator response (HTTP body). Secrets are never echoed back in events.
#[derive(Debug, Clone, Deserialize)]
pub struct HumanInputRespondBody {
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub selected: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    /// Secret value — accepted only for `kind=secret`; never logged.
    #[serde(default)]
    pub secret: Option<String>,
}

/// Result returned to the agent tool (never includes raw secrets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanInputOutcome {
    Cancelled,
    TimedOut,
    Choice(String),
    Text(String),
    /// Opaque slot id; consume via shell `secret_slot`.
    SecretSlot(String),
    /// Operator confirmed they handled the handoff externally.
    HandoffDone,
}

struct PendingEntry {
    kind: HumanInputKind,
    respond_tx: oneshot::Sender<HumanInputRespondBody>,
}

/// Shared hub for interactive human input prompts.
#[derive(Clone)]
pub struct HumanInputHub {
    pending: Arc<Mutex<HashMap<String, PendingEntry>>>,
    events: broadcast::Sender<HumanInputRequiredEvent>,
    secret_slots: Arc<SecretSlotStore>,
}

impl HumanInputHub {
    pub fn new(secret_slots: Arc<SecretSlotStore>) -> Self {
        let (events, _) = broadcast::channel(32);
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            events,
            secret_slots,
        }
    }

    pub fn secret_slots(&self) -> Arc<SecretSlotStore> {
        Arc::clone(&self.secret_slots)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HumanInputRequiredEvent> {
        self.events.subscribe()
    }

    pub async fn request(&self, request: HumanInputRequest) -> HumanInputOutcome {
        if request.kind == HumanInputKind::Choice && request.options.is_empty() {
            return HumanInputOutcome::Cancelled;
        }

        let kind = request.kind;
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        {
            self.pending.lock().insert(
                id.clone(),
                PendingEntry {
                    kind,
                    respond_tx: tx,
                },
            );
        }

        let event = HumanInputRequiredEvent {
            id: id.clone(),
            kind,
            prompt: request.prompt,
            options: request.options,
            risk_note: request.risk_note,
        };
        if self.events.send(event).is_err() {
            tracing::warn!(
                input_id = %id,
                "human_input_required broadcast has no subscribers"
            );
        }

        let body = match tokio::time::timeout(DEFAULT_INPUT_TIMEOUT, rx).await {
            Ok(Ok(body)) => body,
            Ok(Err(_)) => {
                self.pending.lock().remove(&id);
                return HumanInputOutcome::TimedOut;
            }
            Err(_) => {
                self.pending.lock().remove(&id);
                tracing::warn!(input_id = %id, "human input timed out");
                return HumanInputOutcome::TimedOut;
            }
        };

        if body.cancelled {
            return HumanInputOutcome::Cancelled;
        }

        match kind {
            HumanInputKind::Choice => body
                .selected
                .filter(|s| !s.is_empty())
                .map(HumanInputOutcome::Choice)
                .unwrap_or(HumanInputOutcome::Cancelled),
            HumanInputKind::Text => body
                .text
                .filter(|s| !s.is_empty())
                .map(HumanInputOutcome::Text)
                .unwrap_or(HumanInputOutcome::Cancelled),
            HumanInputKind::Secret => {
                let Some(secret) = body.secret.filter(|s| !s.is_empty()) else {
                    return HumanInputOutcome::Cancelled;
                };
                let slot = self.secret_slots.put(secret);
                HumanInputOutcome::SecretSlot(slot)
            }
            HumanInputKind::Handoff => HumanInputOutcome::HandoffDone,
        }
    }

    /// Resolve a pending prompt from HTTP.
    pub fn respond(&self, id: &str, body: HumanInputRespondBody) -> bool {
        let entry = self.pending.lock().remove(id);
        let Some(entry) = entry else {
            return false;
        };
        let ok = match entry.kind {
            HumanInputKind::Choice => {
                body.cancelled || body.selected.as_ref().is_some_and(|s| !s.is_empty())
            }
            HumanInputKind::Text => {
                body.cancelled || body.text.as_ref().is_some_and(|s| !s.is_empty())
            }
            HumanInputKind::Secret => {
                body.cancelled || body.secret.as_ref().is_some_and(|s| !s.is_empty())
            }
            HumanInputKind::Handoff => true,
        };
        if !ok {
            let _ = entry.respond_tx.send(HumanInputRespondBody {
                cancelled: true,
                selected: None,
                text: None,
                secret: None,
            });
            return false;
        }
        entry.respond_tx.send(body).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn choice_response_unblocks_request() {
        let slots = Arc::new(SecretSlotStore::new());
        let hub = HumanInputHub::new(slots);
        let mut sub = hub.subscribe();
        let hub2 = hub.clone();

        let waiter = tokio::spawn(async move {
            hub2.request(HumanInputRequest {
                kind: HumanInputKind::Choice,
                prompt: "Pick one".into(),
                options: vec!["handoff".into(), "secret".into()],
                risk_note: None,
            })
            .await
        });

        let ev = tokio::time::timeout(Duration::from_secs(2), sub.recv())
            .await
            .expect("event")
            .expect("recv");
        assert_eq!(ev.kind, HumanInputKind::Choice);
        assert!(hub.respond(
            &ev.id,
            HumanInputRespondBody {
                cancelled: false,
                selected: Some("handoff".into()),
                text: None,
                secret: None,
            }
        ));
        assert_eq!(
            waiter.await.expect("join"),
            HumanInputOutcome::Choice("handoff".into())
        );
    }

    #[tokio::test]
    async fn secret_response_creates_one_shot_slot() {
        let slots = Arc::new(SecretSlotStore::new());
        let hub = HumanInputHub::new(Arc::clone(&slots));
        let mut sub = hub.subscribe();
        let hub2 = hub.clone();

        let waiter = tokio::spawn(async move {
            hub2.request(HumanInputRequest {
                kind: HumanInputKind::Secret,
                prompt: "sudo password".into(),
                options: vec![],
                risk_note: Some("sent to local daemon only".into()),
            })
            .await
        });

        let ev = tokio::time::timeout(Duration::from_secs(2), sub.recv())
            .await
            .expect("event")
            .expect("recv");
        assert!(hub.respond(
            &ev.id,
            HumanInputRespondBody {
                cancelled: false,
                selected: None,
                text: None,
                secret: Some("pw".into()),
            }
        ));
        match waiter.await.expect("join") {
            HumanInputOutcome::SecretSlot(id) => {
                assert_eq!(slots.take(&id).as_deref(), Some("pw"));
                assert!(slots.take(&id).is_none());
            }
            other => panic!("expected SecretSlot, got {other:?}"),
        }
    }
}
