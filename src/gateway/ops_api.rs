//! Ops API: cron, tools catalog, approvals, provider connectivity (VL-UI-004).
//! 运维 API：定时任务、工具目录、审批、提供商连通性（VL-UI-004）。

use super::local_control::auth::check_pairing_auth;
use super::{client_key_from_request, AppState};
use crate::cron::scheduler::execute_job_now;
use crate::cron::{
    add_shell_job, get_job, list_jobs, remove_job, update_job, CronJob, CronJobPatch, Schedule,
};
use crate::providers::{self, ChatMessage, ChatRequest};
use crate::runtime;
use crate::security::SecurityPolicy;
use crate::tools::{self, Tool};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": message })))
}

fn authorize(
    state: &AppState,
    peer_addr: SocketAddr,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let rate_key = client_key_from_request(Some(peer_addr), headers, state.trust_forwarded_headers);
    if !state.rate_limiter.allow_webhook(&rate_key) {
        return Err(api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests. Please retry later.",
        ));
    }
    check_pairing_auth(&state.pairing, headers, None).map_err(|r| {
        (
            r.status(),
            Json(serde_json::json!({ "error": "Unauthorized" })),
        )
    })?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreateCronBody {
    pub expression: String,
    #[serde(default)]
    pub tz: Option<String>,
    pub command: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateCronBody {
    #[serde(default)]
    pub expression: Option<String>,
    #[serde(default)]
    pub tz: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
struct CronListResponse {
    jobs: Vec<CronJob>,
}

pub async fn handle_list_cron(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    match list_jobs(&config) {
        Ok(jobs) => (StatusCode::OK, Json(CronListResponse { jobs })).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

pub async fn handle_create_cron(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<CreateCronBody>,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let schedule = Schedule::Cron {
        expr: body.expression,
        tz: body.tz,
    };
    match add_shell_job(&config, body.name, schedule, &body.command) {
        Ok(job) => (StatusCode::CREATED, Json(job)).into_response(),
        Err(e) => api_error(StatusCode::BAD_REQUEST, &e.to_string()).into_response(),
    }
}

pub async fn handle_get_cron(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    match get_job(&config, &id) {
        Ok(job) => (StatusCode::OK, Json(job)).into_response(),
        Err(e) => api_error(StatusCode::NOT_FOUND, &e.to_string()).into_response(),
    }
}

pub async fn handle_update_cron(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateCronBody>,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let schedule = body
        .expression
        .map(|expr| Schedule::Cron { expr, tz: body.tz });
    let patch = CronJobPatch {
        schedule,
        command: body.command,
        name: body.name,
        enabled: body.enabled,
        ..CronJobPatch::default()
    };
    match update_job(&config, &id, &patch) {
        Ok(job) => (StatusCode::OK, Json(job)).into_response(),
        Err(e) => api_error(StatusCode::BAD_REQUEST, &e.to_string()).into_response(),
    }
}

pub async fn handle_delete_cron(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    match remove_job(&config, &id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(StatusCode::NOT_FOUND, &e.to_string()).into_response(),
    }
}

#[derive(Debug, Serialize)]
struct CronRunResponse {
    success: bool,
    output: String,
}

pub async fn handle_run_cron(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let job = match get_job(&config, &id) {
        Ok(j) => j,
        Err(e) => return api_error(StatusCode::NOT_FOUND, &e.to_string()).into_response(),
    };
    let (success, output) = execute_job_now(&config, &job).await;
    (StatusCode::OK, Json(CronRunResponse { success, output })).into_response()
}

#[derive(Debug, Serialize)]
struct ToolCatalogEntry {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ToolsListResponse {
    tools: Vec<ToolCatalogEntry>,
}

pub async fn handle_list_tools(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let runtime: Arc<dyn runtime::RuntimeAdapter> = match runtime::create_runtime(&config.runtime) {
        Ok(r) => Arc::from(r),
        Err(e) => {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response();
        }
    };
    let config_arc = Arc::new(config.clone());
    let tool_list = tools::all_tools_with_runtime(
        config_arc,
        &security,
        runtime,
        state.mem.clone(),
        config.composio.api_key.as_deref(),
        config.composio.entity_id.as_deref(),
        &config.browser,
        &config.http_request,
        &config.workspace_dir,
        &config.agents,
        config.api_key.as_deref(),
        &config,
    );
    let tools: Vec<ToolCatalogEntry> = tool_list
        .iter()
        .map(|t| {
            let spec = t.spec();
            ToolCatalogEntry {
                name: spec.name,
                description: spec.description,
                parameters: spec.parameters,
            }
        })
        .collect();
    (StatusCode::OK, Json(ToolsListResponse { tools })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ApprovalRespondBody {
    pub approved: bool,
    #[serde(default)]
    pub always: bool,
}

pub async fn handle_respond_approval(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ApprovalRespondBody>,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    if state.approval_hub.respond(&id, body.approved, body.always) {
        (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
    } else {
        api_error(StatusCode::NOT_FOUND, "Unknown or expired approval id").into_response()
    }
}

#[derive(Debug, Serialize)]
struct ProviderTestResponse {
    ok: bool,
    provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub async fn handle_test_provider(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = authorize(&state, peer_addr, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let provider: Arc<dyn providers::Provider> =
        match providers::create_resilient_provider_with_options(
            &provider_id,
            config.api_key.as_deref(),
            config.api_url.as_deref(),
            &config.reliability,
            &providers::ProviderRuntimeOptions {
                auth_profile_override: None,
                velaclaw_dir: config.config_path.parent().map(std::path::PathBuf::from),
                secrets_encrypt: config.secrets.encrypt,
                reasoning_enabled: config.runtime.reasoning_enabled,
            },
            None,
        ) {
            Ok(p) => Arc::from(p),
            Err(e) => {
                return (
                    StatusCode::OK,
                    Json(ProviderTestResponse {
                        ok: false,
                        provider_id,
                        message: Some(e.to_string()),
                    }),
                )
                    .into_response();
            }
        };

    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| format!("{provider_id}/default"));
    let req = ChatRequest {
        messages: &[ChatMessage::user("ping")],
        tools: None,
    };
    match provider.chat(req, &model, 0.0).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ProviderTestResponse {
                ok: true,
                provider_id,
                message: Some("Connectivity OK".into()),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::OK,
            Json(ProviderTestResponse {
                ok: false,
                provider_id,
                message: Some(e.to_string()),
            }),
        )
            .into_response(),
    }
}
