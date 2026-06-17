//! GET `/ws` — WebSocket streaming chat via agent loop (VL-UI-002).
//! GET `/ws` — 经 agent 循环的 WebSocket 流式对话（VL-UI-002）。

use super::local_control::{
    check_pairing_auth, chunk_text_for_stream, run_agent_chat, unauthorized_response,
    ChatApiRequest, WsClientMessage, WsServerMessage,
};
use super::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

const WS_CHUNK_SIZE: usize = 48;

#[derive(Debug, Deserialize, Default)]
pub struct WsQuery {
    #[serde(default)]
    pub token: Option<String>,
}

/// GET /ws — upgrade to WebSocket for streaming chat.
pub async fn handle_ws_chat(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    if check_pairing_auth(
        &state.pairing,
        &headers,
        query.token.as_deref(),
    )
    .is_err()
    {
        return unauthorized_response().into_response();
    }

    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn handle_ws_socket(mut socket: WebSocket, state: AppState) {
    while let Some(msg) = socket.next().await {
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
                if send_frame(&mut socket, &frame).await.is_err() {
                    break;
                }
                continue;
            }
        };

        if client.msg_type != "chat" {
            let frame = WsServerMessage::Error {
                message: format!("Unsupported message type: {}", client.msg_type),
            };
            if send_frame(&mut socket, &frame).await.is_err() {
                break;
            }
            continue;
        }

        if client.messages.is_empty() {
            let frame = WsServerMessage::Error {
                message: "messages must not be empty".into(),
            };
            if send_frame(&mut socket, &frame).await.is_err() {
                break;
            }
            continue;
        }

        let req = ChatApiRequest {
            messages: client.messages,
            model_id: client.model_id,
            temperature: client.temperature,
            max_tokens: None,
        };

        let config = state.config.lock().clone();
        match run_agent_chat(&config, &req).await {
            Ok(resp) => {
                for chunk in chunk_text_for_stream(&resp.content, WS_CHUNK_SIZE) {
                    let delta = WsServerMessage::Delta { content: chunk };
                    if send_frame(&mut socket, &delta).await.is_err() {
                        return;
                    }
                }
                let done = WsServerMessage::Done {
                    usage: resp.usage,
                    cost: resp.cost,
                };
                if send_frame(&mut socket, &done).await.is_err() {
                    return;
                }
            }
            Err(e) => {
                let frame = WsServerMessage::Error {
                    message: e.to_string(),
                };
                if send_frame(&mut socket, &frame).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn send_frame(socket: &mut WebSocket, frame: &WsServerMessage) -> Result<(), ()> {
    let text = serde_json::to_string(frame).map_err(|_| ())?;
    socket.send(Message::Text(text.into())).await.map_err(|_| ())
}
