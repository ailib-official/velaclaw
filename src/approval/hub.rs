//! Gateway-scoped pending tool approvals (VL-UI-004).
//! 网关侧待审批工具调用队列（VL-UI-004）。

use super::{ApprovalRequest, ApprovalResponse};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;

const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Event emitted to WebSocket clients when a tool needs user approval.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ApprovalRequiredEvent {
    pub id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub arguments_summary: String,
    /// Post-execute elevation after sandbox/policy miss (VL-SEC-011).
    #[serde(default)]
    pub elevation: bool,
}

struct PendingEntry {
    respond_tx: oneshot::Sender<ApprovalResponse>,
}

/// Shared registry for interactive web approvals.
#[derive(Clone)]
pub struct ApprovalHub {
    pending: Arc<Mutex<HashMap<String, PendingEntry>>>,
    events: broadcast::Sender<ApprovalRequiredEvent>,
}

impl ApprovalHub {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(32);
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApprovalRequiredEvent> {
        self.events.subscribe()
    }

    /// Register a pending approval and wait for `respond` or timeout.
    pub async fn request(&self, request: &ApprovalRequest, summary: &str) -> ApprovalResponse {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.pending.lock();
            guard.insert(id.clone(), PendingEntry { respond_tx: tx });
        }

        let event = ApprovalRequiredEvent {
            id: id.clone(),
            tool_name: request.tool_name.clone(),
            arguments: request.arguments.clone(),
            arguments_summary: summary.to_string(),
            elevation: request.elevation,
        };
        let receivers = self.events.receiver_count();
        match self.events.send(event) {
            Ok(n) => {
                tracing::debug!(
                    approval_id = %id,
                    tool = %request.tool_name,
                    delivered_to = n,
                    "approval_required broadcast"
                );
            }
            Err(_) => {
                tracing::warn!(
                    approval_id = %id,
                    tool = %request.tool_name,
                    receivers,
                    "approval_required broadcast has no subscribers; UI will not show a modal"
                );
            }
        }

        match tokio::time::timeout(DEFAULT_APPROVAL_TIMEOUT, rx).await {
            Ok(Ok(decision)) => decision,
            Ok(Err(_)) => {
                tracing::warn!(
                    approval_id = %id,
                    "approval oneshot closed without respond (treating as deny)"
                );
                self.pending.lock().remove(&id);
                ApprovalResponse::No
            }
            Err(_) => {
                tracing::warn!(
                    approval_id = %id,
                    timeout_secs = DEFAULT_APPROVAL_TIMEOUT.as_secs(),
                    "approval timed out waiting for UI respond"
                );
                self.pending.lock().remove(&id);
                ApprovalResponse::No
            }
        }
    }

    /// Drop all waiting approvals (turn Stop). Callers must treat this as cancel, not user deny.
    pub fn abort_all_pending(&self) {
        self.pending.lock().clear();
    }

    /// Resolve a pending approval from HTTP (`POST /api/approvals/:id/respond`).
    pub fn respond(&self, id: &str, approved: bool, always: bool, never: bool) -> bool {
        let entry = self.pending.lock().remove(id);
        let Some(entry) = entry else {
            return false;
        };
        let decision = if never {
            ApprovalResponse::Never
        } else if approved {
            if always {
                ApprovalResponse::Always
            } else {
                ApprovalResponse::Yes
            }
        } else {
            ApprovalResponse::No
        };
        entry.respond_tx.send(decision).is_ok()
    }
}

impl Default for ApprovalHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn respond_unblocks_request() {
        let hub = ApprovalHub::new();
        let hub_wait = hub.clone();
        let req = ApprovalRequest {
            tool_name: "shell".into(),
            arguments: serde_json::json!({"command": "ls"}),
            elevation: false,
        };

        let handle = tokio::spawn(async move { hub_wait.request(&req, "command: ls").await });

        tokio::time::sleep(Duration::from_millis(20)).await;
        let pending: Vec<_> = hub.pending.lock().keys().cloned().collect();
        assert_eq!(pending.len(), 1);
        assert!(hub.respond(&pending[0], true, false, false));

        let decision = handle.await.expect("join");
        assert_eq!(decision, ApprovalResponse::Yes);
    }

    #[tokio::test]
    async fn abort_all_pending_unblocks_request() {
        let hub = ApprovalHub::new();
        let hub_wait = hub.clone();
        let req = ApprovalRequest {
            tool_name: "shell".into(),
            arguments: serde_json::json!({"command": "ls"}),
            elevation: false,
        };

        let handle = tokio::spawn(async move { hub_wait.request(&req, "command: ls").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(hub.pending.lock().len(), 1);
        hub.abort_all_pending();
        let decision = handle.await.expect("join");
        assert_eq!(decision, ApprovalResponse::No);
        assert!(hub.pending.lock().is_empty());
    }
}
