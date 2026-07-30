//! Gateway HTTP router construction (VL-REVIEW-004).

use super::{
    chat_api, chat_ws, config_api, handle_dashboard, handle_dashboard_api, handle_health,
    handle_linq_webhook, handle_metrics, handle_nextcloud_talk_webhook, handle_pair,
    handle_webhook, handle_whatsapp_message, handle_whatsapp_verify, memory_api, ops_api,
    providers_api, sessions_api, static_embed, AppState, MAX_BODY_SIZE, REQUEST_TIMEOUT_SECS,
};
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::time::Duration;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

/// Build the axum router for the gateway HTTP surface.
pub fn build_gateway_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handle_health))
        .route("/metrics", get(handle_metrics))
        .route("/dashboard", get(handle_dashboard))
        .route("/api/dashboard", get(handle_dashboard_api))
        .route("/api/chat", post(chat_api::handle_post_chat))
        .route("/api/providers", get(providers_api::handle_get_providers))
        .route("/api/sessions", get(sessions_api::handle_list_sessions))
        .route("/api/sessions", post(sessions_api::handle_create_session))
        .route("/api/sessions/{id}", get(sessions_api::handle_get_session))
        .route(
            "/api/sessions/{id}",
            delete(sessions_api::handle_delete_session),
        )
        .route("/api/memory", get(memory_api::handle_list_memory))
        .route("/api/memory/{id}", get(memory_api::handle_get_memory))
        .route("/api/config", get(config_api::handle_get_config))
        .route("/api/config", put(config_api::handle_put_config))
        .route(
            "/api/config/schema",
            get(config_api::handle_get_config_schema),
        )
        .route("/api/cron", get(ops_api::handle_list_cron))
        .route("/api/cron", post(ops_api::handle_create_cron))
        .route("/api/cron/{id}", get(ops_api::handle_get_cron))
        .route("/api/cron/{id}", put(ops_api::handle_update_cron))
        .route("/api/cron/{id}", delete(ops_api::handle_delete_cron))
        .route("/api/cron/{id}/run", post(ops_api::handle_run_cron))
        .route("/api/tools", get(ops_api::handle_list_tools))
        .route(
            "/api/providers/{id}/test",
            post(ops_api::handle_test_provider),
        )
        .route(
            "/api/approvals/{id}/respond",
            post(ops_api::handle_respond_approval),
        )
        .route("/ws", get(chat_ws::handle_ws_chat))
        .route("/chat", get(static_embed::handle_chat_ui))
        .route("/chat/{*asset_path}", get(static_embed::handle_chat_ui))
        .route("/pair", post(handle_pair))
        .route("/webhook", post(handle_webhook))
        .route("/whatsapp", get(handle_whatsapp_verify))
        .route("/whatsapp", post(handle_whatsapp_message))
        .route("/linq", post(handle_linq_webhook))
        .route("/nextcloud-talk", post(handle_nextcloud_talk_webhook))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(REQUEST_TIMEOUT_SECS),
        ))
}

/// Options controlling which webhook endpoints appear in the listen banner.
pub struct GatewayBannerChannels {
    pub whatsapp: bool,
    pub linq: bool,
    pub nextcloud: bool,
}

/// Print the gateway listen banner and endpoint summary.
pub fn print_gateway_banner(
    display_addr: &str,
    tunnel_url: Option<&str>,
    channels: GatewayBannerChannels,
    pairing_code: Option<&str>,
    require_pairing: bool,
) {
    println!("🦀 VelaClaw Gateway listening on http://{display_addr}");
    if let Some(url) = tunnel_url {
        println!("  🌐 Public URL: {url}");
    }
    println!("  POST /pair      — pair a new client (X-Pairing-Code header)");
    println!("  POST /webhook   — {{\"message\": \"your prompt\"}}");
    if channels.whatsapp {
        println!("  GET  /whatsapp  — Meta webhook verification");
        println!("  POST /whatsapp  — WhatsApp message webhook");
    }
    if channels.linq {
        println!("  POST /linq      — Linq message webhook (iMessage/RCS/SMS)");
    }
    if channels.nextcloud {
        println!("  POST /nextcloud-talk — Nextcloud Talk bot webhook");
    }
    println!("  GET  /health    — health check");
    println!("  GET  /metrics   — Prometheus metrics");
    println!("  GET  /chat         — Web UI (Overview tab: cost, runtime, status)");
    println!("  GET  /dashboard    — redirects to /chat/?tab=overview");
    if let Some(code) = pairing_code {
        println!();
        println!("  🔐 PAIRING REQUIRED — use this one-time code:");
        println!("     ┌──────────────┐");
        println!("     │  {code}  │");
        println!("     └──────────────┘");
        println!("     Send: POST /pair with header X-Pairing-Code: {code}");
    } else if require_pairing {
        println!("  🔒 Pairing: ACTIVE (bearer token required)");
    } else {
        println!("  ⚠️  Pairing: DISABLED (all requests accepted)");
    }
    println!("  Press Ctrl+C to stop.\n");
}
