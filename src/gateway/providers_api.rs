//! GET `/api/providers` — protocol registry metadata (VL-UI-002).
//! GET `/api/providers` — 协议注册表元数据（VL-UI-002）。

use super::local_control::{
    check_pairing_auth, ModelApiEntry, ProviderApiEntry, ProvidersApiResponse,
};
use super::{client_key_from_request, AppState, RATE_LIMIT_WINDOW_SECS};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use std::collections::HashMap;
use std::net::SocketAddr;

/// GET /api/providers — list logical models and provider availability (no secrets).
pub async fn handle_get_providers(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let rate_key =
        client_key_from_request(Some(peer_addr), &headers, state.trust_forwarded_headers);
    if !state.rate_limiter.allow_webhook(&rate_key) {
        let err = serde_json::json!({
            "error": "Too many requests. Please retry later.",
            "retry_after": RATE_LIMIT_WINDOW_SECS,
        });
        return (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
    }

    if let Err(response) = check_pairing_auth(&state.pairing, &headers, None) {
        return response.into_response();
    }

    let root = match crate::protocol_registry::resolve_local_protocol_root() {
        Some(r) => r,
        None => {
            let body = ProvidersApiResponse {
                providers: vec![],
                models: vec![],
            };
            return (StatusCode::OK, Json(body)).into_response();
        }
    };

    match crate::protocol_registry::scan_protocol_root(&root) {
        Ok(snapshot) => {
            let provider_availability: HashMap<String, bool> = snapshot
                .providers
                .iter()
                .map(|p| (p.id.clone(), p.available))
                .collect();

            let providers: Vec<ProviderApiEntry> = snapshot
                .providers
                .into_iter()
                .map(|p| ProviderApiEntry {
                    id: p.id,
                    available: p.available,
                    required_envs: p.required_envs,
                })
                .collect();

            let models: Vec<ModelApiEntry> = snapshot
                .models
                .into_iter()
                .map(|m| {
                    let available = provider_availability
                        .get(&m.provider)
                        .copied()
                        .unwrap_or(false);
                    ModelApiEntry {
                        logical_id: m.logical_id,
                        provider: m.provider,
                        available,
                    }
                })
                .collect();

            let body = ProvidersApiResponse { providers, models };
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => {
            tracing::warn!("GET /api/providers scan failed: {e:#}");
            let err = serde_json::json!({ "error": e.to_string() });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}
