//! Memory browser API for Web Chat Phase 2 (VL-UI-003).
//! Web Chat 第二阶段记忆浏览 API（VL-UI-003）。

use super::local_control::auth::check_pairing_auth;
use super::{client_key_from_request, AppState, RATE_LIMIT_WINDOW_SECS};
use crate::memory::MemoryEntry;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use serde::Deserialize;
use std::net::SocketAddr;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

#[derive(Debug, Deserialize, Default)]
pub struct MemoryListQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

/// HTTP view of [`MemoryEntry`]. `category` is the Display string so custom
/// names stay JSON strings (not `{ "Custom": "…" }`).
#[derive(Debug, serde::Serialize)]
struct MemoryApiEntry {
    id: String,
    key: String,
    content: String,
    category: String,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<f64>,
}

impl From<MemoryEntry> for MemoryApiEntry {
    fn from(entry: MemoryEntry) -> Self {
        Self {
            id: entry.id,
            key: entry.key,
            content: entry.content,
            category: entry.category.to_string(),
            timestamp: entry.timestamp,
            session_id: entry.session_id,
            score: entry.score,
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct MemoryListResponse {
    entries: Vec<MemoryApiEntry>,
    total: usize,
    limit: usize,
    offset: usize,
}

/// GET /api/memory — paginated memory search.
pub async fn handle_list_memory(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<MemoryListQuery>,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0);

    let result = if let Some(q) = query.q.as_deref().filter(|s| !s.trim().is_empty()) {
        state.mem.recall(q.trim(), limit + offset, None).await
    } else {
        state.mem.list(None, None).await
    };

    match result {
        Ok(mut entries) => {
            for entry in &mut entries {
                entry.content = redact_memory_content(&entry.content);
            }
            let total = entries.len();
            let page: Vec<MemoryApiEntry> = entries
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(MemoryApiEntry::from)
                .collect();
            let body = MemoryListResponse {
                entries: page,
                total,
                limit,
                offset,
            };
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => {
            tracing::warn!("GET /api/memory failed: {e:#}");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    }
}

/// GET /api/memory/:id — single memory entry by key/id.
pub async fn handle_get_memory(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    match state.mem.get(&id).await {
        Ok(Some(mut entry)) => {
            entry.content = redact_memory_content(&entry.content);
            (StatusCode::OK, Json(MemoryApiEntry::from(entry))).into_response()
        }
        Ok(None) => api_error(StatusCode::NOT_FOUND, "memory entry not found").into_response(),
        Err(e) => {
            tracing::warn!("GET /api/memory/{id} failed: {e:#}");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    }
}

fn redact_memory_content(content: &str) -> String {
    let mut out = content.to_string();
    for marker in ["sk-", "Bearer ", "api_key=", "password="] {
        if out.contains(marker) {
            out = out.replace(marker, "[redacted]");
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_memory_content_masks_sk_prefix() {
        let input = "token sk-abcdef1234567890 here";
        let out = redact_memory_content(input);
        assert!(!out.contains("sk-abcdef"));
        assert!(out.contains("[redacted]"));
    }

    #[test]
    fn memory_list_matches_ui_golden_fixture() {
        use crate::memory::{MemoryCategory, MemoryEntry};

        let entry = MemoryEntry {
            id: "mem-1".into(),
            key: "project".into(),
            content: "notes".into(),
            category: MemoryCategory::Custom("project_notes".into()),
            timestamp: "2026-09-12T00:00:00Z".into(),
            session_id: Some("sess-1".into()),
            score: Some(0.91),
        };
        let body = MemoryListResponse {
            entries: vec![MemoryApiEntry::from(entry)],
            total: 1,
            limit: 50,
            offset: 0,
        };
        let actual = serde_json::to_value(&body).expect("serialize");
        let path = format!(
            "{}/ui-chat/src/lib/fixtures/memory_list.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let golden: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture"))
                .expect("parse golden");
        assert_eq!(actual, golden);
    }
}
