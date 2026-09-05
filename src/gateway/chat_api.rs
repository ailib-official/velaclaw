//! POST `/api/chat` — non-streaming agent-loop chat (VL-UI-002).
//! POST `/api/chat` — 非流式 agent 循环对话（VL-UI-002）。
//!
//! **No turn-cancel:** this handler passes `cancellation = None`. Stop a running
//! turn via WebSocket `GET /ws` (`type: cancel` or Close). See
//! `docs/cancel-contract.md`. Do not add a second REST cancel protocol.

use super::local_control::auth::check_pairing_auth;
use super::local_control::runner::{persist_chat_turn, run_agent_chat, user_facing_turn_error};
use super::local_control::types::ChatApiRequest;
use super::{client_key_from_request, AppState, RATE_LIMIT_WINDOW_SECS};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use std::net::SocketAddr;

/// POST /api/chat — run one agent turn and return the full assistant reply.
pub async fn handle_post_chat(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<ChatApiRequest>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    let rate_key =
        client_key_from_request(Some(peer_addr), &headers, state.trust_forwarded_headers);
    if !state.rate_limiter.allow_webhook(&rate_key) {
        let err = serde_json::json!({
            "error": "Too many chat requests. Please retry later.",
            "retry_after": RATE_LIMIT_WINDOW_SECS,
        });
        return (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
    }

    if let Err(response) = check_pairing_auth(&state.pairing, &headers, None) {
        return response.into_response();
    }

    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => {
            let err = serde_json::json!({
                "error": format!("Invalid JSON body: {e}"),
            });
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    };

    if req.messages.is_empty() {
        let err = serde_json::json!({
            "error": "messages must not be empty",
        });
        return (StatusCode::BAD_REQUEST, Json(err)).into_response();
    }

    let config = state.config.lock().clone();
    match run_agent_chat(
        &config,
        &req,
        Some(&state.approval_hub),
        Some(&state.human_input_hub),
        None,
        None,
    )
    .await
    {
        Ok(resp) => {
            if let Err(e) = persist_chat_turn(
                &config,
                req.session_id.as_deref(),
                &req,
                &resp.content,
                Some(state.session_title_hub.clone()),
            )
            .await
            {
                tracing::warn!("session persist failed: {e:#}");
            }
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::warn!("POST /api/chat failed: {e:#}");
            let err = serde_json::json!({
                "error": user_facing_turn_error(&e, req.model_id.as_deref()),
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}
