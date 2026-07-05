//! Embedded Web Chat SPA served at `/chat` (VL-UI-005).
//! 在 `/chat` 提供内嵌 Web Chat SPA（VL-UI-005）。

use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[cfg(chat_ui_dist)]
#[derive(Embed)]
#[folder = "ui-chat/dist/"]
struct ChatUiDistAssets;

#[cfg(not(chat_ui_dist))]
#[derive(Embed)]
#[folder = "ui-chat/embed-stub/"]
struct ChatUiStubAssets;

#[cfg(chat_ui_dist)]
type ChatUiAssets = ChatUiDistAssets;

#[cfg(not(chat_ui_dist))]
type ChatUiAssets = ChatUiStubAssets;

fn resolve_embed_path(uri_path: &str) -> String {
    let trimmed = uri_path
        .strip_prefix("/chat")
        .unwrap_or(uri_path)
        .trim_start_matches('/');
    if trimmed.is_empty() {
        "index.html".to_string()
    } else {
        trimmed.to_string()
    }
}

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".js") {
        "application/javascript"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "text/html; charset=utf-8"
    }
}

fn serve_embedded(relative_path: &str) -> Response {
    let Some(file) = ChatUiAssets::get(relative_path) else {
        if relative_path != "index.html" {
            return serve_embedded("index.html");
        }
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "chat UI asset not found",
        )
            .into_response();
    };

    let ct = content_type(relative_path);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, ct)],
        file.data.into_owned(),
    )
        .into_response()
}

/// Serve SPA and static assets under `/chat` and `/chat/*`.
pub async fn handle_chat_ui(req: Request) -> impl IntoResponse {
    serve_embedded(&resolve_embed_path(req.uri().path()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_index_exists() {
        assert!(ChatUiAssets::get("index.html").is_some());
    }

    #[test]
    fn maps_chat_root_to_index() {
        assert_eq!(resolve_embed_path("/chat"), "index.html");
        assert_eq!(resolve_embed_path("/chat/"), "index.html");
        assert_eq!(resolve_embed_path("/chat/assets/app.js"), "assets/app.js");
    }
}
