//! Broadcast refined chat-session titles to open WebSockets.
//! 将会话标题 refinement 推到已打开的 WebSocket。

use tokio::sync::broadcast;

/// Event after a session title is written to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTitleEvent {
    pub session_id: String,
    pub title: String,
}

/// Gateway-scoped title bus (same pattern as ApprovalHub events).
#[derive(Clone)]
pub struct SessionTitleHub {
    events: broadcast::Sender<SessionTitleEvent>,
}

impl SessionTitleHub {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(32);
        Self { events }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SessionTitleEvent> {
        self.events.subscribe()
    }

    /// Notify listeners; no-op when nobody is connected.
    pub fn publish(&self, session_id: &str, title: &str) {
        let event = SessionTitleEvent {
            session_id: session_id.to_string(),
            title: title.to_string(),
        };
        let _ = self.events.send(event);
    }
}

impl Default for SessionTitleHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_reaches_subscriber() {
        let hub = SessionTitleHub::new();
        let mut rx = hub.subscribe();
        hub.publish("sess-1", "LAN Scan");
        let ev = rx.recv().await.expect("event");
        assert_eq!(ev.session_id, "sess-1");
        assert_eq!(ev.title, "LAN Scan");
    }
}
