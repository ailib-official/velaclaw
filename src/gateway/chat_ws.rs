//! GET `/ws` — WebSocket streaming chat via agent loop (VL-UI-002).
//! GET `/ws` — 经 agent 循环的 WebSocket 流式对话（VL-UI-002）。

use super::local_control::auth::{check_pairing_auth, unauthorized_response};
use super::local_control::runner::{chunk_text_for_stream, persist_chat_turn, run_agent_chat};
use super::local_control::types::{ChatApiRequest, WsClientMessage, WsServerMessage};
use super::AppState;
use crate::approval::HumanInputKind;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use futures_util::StreamExt;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

const WS_CHUNK_SIZE: usize = 48;

#[derive(Debug, Deserialize, Default)]
pub struct WsQuery {
    #[serde(default)]
    pub token: Option<String>,
}

fn human_input_kind_label(kind: HumanInputKind) -> &'static str {
    match kind {
        HumanInputKind::Choice => "choice",
        HumanInputKind::Text => "text",
        HumanInputKind::Secret => "secret",
        HumanInputKind::Handoff => "handoff",
    }
}

/// GET /ws — upgrade to WebSocket for streaming chat.
pub async fn handle_ws_chat(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    if check_pairing_auth(&state.pairing, &headers, query.token.as_deref()).is_err() {
        return unauthorized_response().into_response();
    }

    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn handle_ws_socket(socket: WebSocket, state: AppState) {
    let socket = Arc::new(Mutex::new(socket));

    while let Some(msg) = {
        let mut guard = socket.lock().await;
        guard.next().await
    } {
        let msg = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(e) => {
                tracing::warn!("WebSocket receive error: {e}");
                break;
            }
        };

        let client: WsClientMessage = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                let frame = WsServerMessage::Error {
                    message: format!("Invalid JSON: {e}"),
                };
                if send_frame(socket.clone(), &frame).await.is_err() {
                    break;
                }
                continue;
            }
        };

        if client.msg_type != "chat" {
            let frame = WsServerMessage::Error {
                message: format!("Unsupported message type: {}", client.msg_type),
            };
            if send_frame(socket.clone(), &frame).await.is_err() {
                break;
            }
            continue;
        }

        if client.messages.is_empty() {
            let frame = WsServerMessage::Error {
                message: "messages must not be empty".into(),
            };
            if send_frame(socket.clone(), &frame).await.is_err() {
                break;
            }
            continue;
        }

        let req = ChatApiRequest {
            messages: client.messages,
            session_id: client.session_id,
            model_id: client.model_id,
            temperature: client.temperature,
            max_tokens: None,
        };

        let config = state.config.lock().clone();
        let hub = state.approval_hub.clone();
        let human_hub = state.human_input_hub.clone();
        let mut approval_sub = hub.subscribe();
        let mut human_sub = human_hub.subscribe();
        let sock_fwd = socket.clone();
        let sock_hitl = socket.clone();
        // Keep forwarding even after `Lagged` (burst of approvals); only stop on closed
        // channel or socket send failure. A bare `while let Ok` exits on Lagged and then
        // silently drops later `approval_required` frames.
        let forwarder = tokio::spawn(async move {
            loop {
                match approval_sub.recv().await {
                    Ok(ev) => {
                        let frame = WsServerMessage::ApprovalRequired {
                            id: ev.id,
                            tool_name: ev.tool_name,
                            arguments_summary: ev.arguments_summary,
                        };
                        if send_frame(sock_fwd.clone(), &frame).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "approval hub subscriber lagged; continuing");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        let hitl_forwarder = tokio::spawn(async move {
            loop {
                match human_sub.recv().await {
                    Ok(ev) => {
                        let frame = WsServerMessage::InputRequired {
                            id: ev.id,
                            kind: human_input_kind_label(ev.kind).to_string(),
                            prompt: ev.prompt,
                            options: ev.options,
                            risk_note: ev.risk_note,
                        };
                        if send_frame(sock_hitl.clone(), &frame).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            skipped = n,
                            "human input hub subscriber lagged; continuing"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let chat_result = run_agent_chat(&config, &req, Some(&hub), Some(&human_hub)).await;
        forwarder.abort();
        hitl_forwarder.abort();

        match chat_result {
            Ok(resp) => {
                if let Err(e) = persist_chat_turn(
                    &config.workspace_dir,
                    req.session_id.as_deref(),
                    &req,
                    &resp.content,
                )
                .await
                {
                    tracing::warn!("session persist failed: {e:#}");
                }
                for chunk in chunk_text_for_stream(&resp.content, WS_CHUNK_SIZE) {
                    let delta = WsServerMessage::Delta { content: chunk };
                    if send_frame(socket.clone(), &delta).await.is_err() {
                        return;
                    }
                }
                let done = WsServerMessage::Done {
                    usage: resp.usage,
                    cost: resp.cost,
                };
                if send_frame(socket.clone(), &done).await.is_err() {
                    return;
                }
            }
            Err(e) => {
                let frame = WsServerMessage::Error {
                    message: e.to_string(),
                };
                if send_frame(socket.clone(), &frame).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn send_frame(socket: Arc<Mutex<WebSocket>>, frame: &WsServerMessage) -> Result<(), ()> {
    let text = serde_json::to_string(frame).map_err(|_| ())?;
    let mut guard = socket.lock().await;
    guard.send(Message::Text(text.into())).await.map_err(|_| ())
}
