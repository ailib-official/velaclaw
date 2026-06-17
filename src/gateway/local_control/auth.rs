//! PairingGuard bearer authentication for Local Control endpoints.
//! 本地控制端点的 PairingGuard Bearer 鉴权。

use crate::security::pairing::PairingGuard;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};

const UNAUTHORIZED_HINT: &str =
    "Unauthorized — pair first via POST /pair, then send Authorization: Bearer <token>";

/// Extract bearer token from `Authorization` header, if present.
pub fn bearer_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Returns `Ok(())` when the request is authorized under current pairing policy.
pub fn check_pairing_auth(
    pairing: &PairingGuard,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !pairing.require_pairing() {
        return Ok(());
    }

    let token = bearer_token_from_headers(headers)
        .or_else(|| {
            query_token
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default();

    if pairing.is_authenticated(&token) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": UNAUTHORIZED_HINT })),
        ))
    }
}

/// Axum response for unauthorized pairing (used before WebSocket upgrade).
pub fn unauthorized_response() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": UNAUTHORIZED_HINT })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_when_pairing_required_and_no_token() {
        let pairing = PairingGuard::new(true, &[]);
        let headers = HeaderMap::new();
        let result = check_pairing_auth(&pairing, &headers, None);
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn allows_when_pairing_not_required() {
        let pairing = PairingGuard::new(false, &[]);
        let headers = HeaderMap::new();
        assert!(check_pairing_auth(&pairing, &headers, None).is_ok());
    }
}
