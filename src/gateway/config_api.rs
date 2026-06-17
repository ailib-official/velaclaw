//! Config read/write API for Web Chat Phase 2 (VL-UI-003).
//! Web Chat 第二阶段配置读写 API（VL-UI-003）。

use super::local_control::auth::check_pairing_auth;
use super::{client_key_from_request, AppState, RATE_LIMIT_WINDOW_SECS};
use crate::config::Config;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use serde_json::{Map, Value};
use std::net::SocketAddr;

const MASK: &str = "****";

/// GET /api/config — masked config snapshot.
pub async fn handle_get_config(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    let config = state.config.lock().clone();
    let value = config_to_masked_value(&config);
    (StatusCode::OK, Json(value)).into_response()
}

/// GET /api/config/schema — JSON Schema for form generation.
pub async fn handle_get_config_schema(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    let schema = schemars::schema_for!(Config);
    let _ = state; // pairing already checked
    (StatusCode::OK, Json(schema)).into_response()
}

/// PUT /api/config — validated partial update (secrets rejected).
pub async fn handle_put_config(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, peer_addr, &headers) {
        return response.into_response();
    }

    let Json(patch) = match body {
        Ok(b) => b,
        Err(e) => {
            return api_error(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}"))
                .into_response()
        }
    };

    if !patch.is_object() {
        return api_error(StatusCode::BAD_REQUEST, "body must be a JSON object").into_response();
    }

    if contains_forbidden_secret_fields(&patch) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "Config PUT cannot set API keys or secrets — use environment variables",
        )
        .into_response();
    }

    let updated = {
        let current = state.config.lock().clone();
        merge_config_patch(&current, &patch)
    };

    match updated {
        Ok(cfg) => {
            if let Err(e) = cfg.save().await {
                tracing::warn!("PUT /api/config save failed: {e:#}");
                return api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
                    .into_response();
            }
            *state.config.lock() = cfg.clone();
            tracing::info!("Config updated via Web Chat settings API");
            let value = config_to_masked_value(&cfg);
            (StatusCode::OK, Json(value)).into_response()
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, &e).into_response(),
    }
}

fn config_to_masked_value(config: &Config) -> Value {
    let mut value = serde_json::to_value(config).unwrap_or_else(|_| Value::Object(Map::new()));
    mask_secret_fields(&mut value);
    value
}

fn mask_secret_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if is_secret_field_name(key) {
                    if child.is_string() && child.as_str().is_some_and(|s| !s.is_empty()) {
                        *child = Value::String(MASK.to_string());
                    }
                } else {
                    mask_secret_fields(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                mask_secret_fields(item);
            }
        }
        _ => {}
    }
}

fn is_secret_field_name(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("api_key")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("token")
        || lower == "app_secret"
}

fn contains_forbidden_secret_fields(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| is_secret_field_name(k) || contains_forbidden_secret_fields(v)),
        Value::Array(items) => items.iter().any(contains_forbidden_secret_fields),
        _ => false,
    }
}

fn merge_config_patch(current: &Config, patch: &Value) -> Result<Config, String> {
    let mut base = serde_json::to_value(current).map_err(|e| e.to_string())?;
    deep_merge(&mut base, patch);
    serde_json::from_value(base).map_err(|e| format!("invalid config after merge: {e}"))
}

fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (key, patch_val) in patch_map {
                if let Some(base_val) = base_map.get_mut(key) {
                    deep_merge(base_val, patch_val);
                } else {
                    base_map.insert(key.clone(), patch_val.clone());
                }
            }
        }
        (base_slot, patch_val) => {
            *base_slot = patch_val.clone();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_secret_patch() {
        let patch = serde_json::json!({ "api_key": "sk-test" });
        assert!(contains_forbidden_secret_fields(&patch));
    }

    #[test]
    fn masks_api_key_in_config_json() {
        let mut cfg = Config::default();
        cfg.api_key = Some("sk-secret".into());
        let mut value = serde_json::to_value(&cfg).unwrap();
        mask_secret_fields(&mut value);
        assert_eq!(value["api_key"], MASK);
    }

    #[test]
    fn merge_updates_default_model() {
        let current = Config::default();
        let patch = serde_json::json!({ "default_model": "deepseek/deepseek-v4-pro" });
        let merged = merge_config_patch(&current, &patch).unwrap();
        assert_eq!(
            merged.default_model.as_deref(),
            Some("deepseek/deepseek-v4-pro")
        );
    }
}
