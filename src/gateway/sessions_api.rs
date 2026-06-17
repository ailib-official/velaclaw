//! Session list/detail API for Web Chat Phase 2 (VL-UI-003).
//! Web Chat 第二阶段会话列表/详情 API（VL-UI-003）。

use super::local_control::auth::check_pairing_auth;
use super::local_control::sessions::ChatSessionStore;
use super::{client_key_from_request, AppState, RATE_LIMIT_WINDOW_SECS};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize)]
pub struct CreateSessionBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
}

fn session_store(state: &AppState) -> ChatSessionStore {
    let workspace = state.config.lock().workspace_dir.clone();
    ChatSessionStore::new(&workspace)
}

/// GET /api/sessions — list chat session summaries.
pub async fn handle_list_sessions(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    match session_store(&state).list().await {
        Ok(sessions) => (
            StatusCode::OK,
            Json(serde_json::json!({ "sessions": sessions })),
        )
            .into_response(),
        Err(e) => {
            tracing::warn!("GET /api/sessions failed: {e:#}");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    }
}

/// POST /api/sessions — create a new chat session.
pub async fn handle_create_session(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<CreateSessionBody>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    let Json(body) = match body {
        Ok(b) => b,
        Err(e) => {
            return api_error(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}"))
                .into_response()
        }
    };

    match session_store(&state)
        .create(body.title, body.model_id)
        .await
    {
        Ok(session) => (StatusCode::CREATED, Json(session)).into_response(),
        Err(e) => {
            tracing::warn!("POST /api/sessions failed: {e:#}");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    }
}

/// GET /api/sessions/:id — session detail with messages.
pub async fn handle_get_session(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    match session_store(&state).get(&id).await {
        Ok(Some(session)) => (StatusCode::OK, Json(session)).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "session not found").into_response(),
        Err(e) => {
            tracing::warn!("GET /api/sessions/{id} failed: {e:#}");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    }
}

/// DELETE /api/sessions/:id — remove a chat session.
pub async fn handle_delete_session(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    match session_store(&state).delete(&id).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))).into_response(),
        Ok(false) => api_error(StatusCode::NOT_FOUND, "session not found").into_response(),
        Err(e) => {
            tracing::warn!("DELETE /api/sessions/{id} failed: {e:#}");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    }
}

fn authorize(
    state: &AppState,
    peer_addr: SocketAddr,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let rate_key = client_key_from_request(Some(peer_addr), headers, state.trust_forwarded_headers);
    if !state.rate_limiter.allow_webhook(&rate_key) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "Too many requests. Please retry later.",
                "retry_after": RATE_LIMIT_WINDOW_SECS,
            })),
        ));
    }

    check_pairing_auth(&state.pairing, headers, None)?;
    Ok(())
}

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": message })))
}
