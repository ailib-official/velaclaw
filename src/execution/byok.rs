//! BYOK execution helpers — AiClient init, transport retry, telemetry (VL-EVO-004).
//! BYOK 执行辅助：客户端初始化、传输层重试与遥测。

use crate::telemetry::ByokTelemetryReporter;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Max retries for retryable protocol errors at the transport layer.
const MAX_RETRIES: u32 = 2;

/// Build an [`ai_lib_rust::AiClient`] for a logical model id (`provider/model`).
pub async fn resolve_ai_client(model_id: &str) -> anyhow::Result<ai_lib_rust::AiClient> {
    ai_lib_rust::AiClient::new(model_id).await.map_err(|e| {
        let base = format!("AiClient::new({model_id}): {e}");
        if model_id.contains('/') {
            anyhow::anyhow!(
                "{base}\n\
                 Hint: logical `provider/model` ids need a local ai-protocol checkout — set `AI_PROTOCOL_DIR` \
                 to the repository root (a directory on disk, not a URL). See `docs/migration-legacy-to-protocol.md`."
            )
        } else {
            anyhow::anyhow!(base)
        }
    })
}

/// Sync AiClient bootstrap for factory paths without an async runtime.
pub fn init_ai_client_sync(model_id: &str) -> anyhow::Result<Arc<ai_lib_rust::AiClient>> {
    let client = if tokio::runtime::Handle::try_current().is_ok() {
        let model_for_thread = model_id.to_string();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async { resolve_ai_client(&model_for_thread).await })
        })
        .join()
        .map_err(|_| anyhow::anyhow!("execution handle initialization thread panicked"))??
    } else {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(resolve_ai_client(model_id))?
    };

    Ok(Arc::new(client))
}

pub fn split_logical_model_id(logical_model_id: &str) -> anyhow::Result<(String, String)> {
    let trimmed = logical_model_id.trim();
    let (provider_id, model_id) = trimmed.split_once('/').ok_or_else(|| {
        anyhow::anyhow!("logical model id must be provider/model, got `{trimmed}`")
    })?;
    if provider_id.is_empty() || model_id.is_empty() {
        anyhow::bail!("logical model id must be provider/model, got `{trimmed}`");
    }
    Ok((provider_id.to_string(), model_id.to_string()))
}

/// Run a chat execute with transport-level retry on retryable ai-lib errors.
pub async fn execute_chat_with_retry(
    client: &ai_lib_rust::AiClient,
    provider_id: &str,
    model_id: &str,
    messages: Vec<ai_lib_rust::Message>,
    temperature: f64,
    tools: Option<Vec<serde_json::Value>>,
    telemetry: Option<&ByokTelemetryReporter>,
) -> Result<ai_lib_rust::client::UnifiedResponse, ai_lib_rust::Error> {
    let started = Instant::now();
    let mut builder = client
        .chat()
        .messages(messages.clone())
        .temperature(temperature);
    if let Some(ref t) = tools {
        if !t.is_empty() {
            builder = builder.tools_json(t.clone());
        }
    }
    let mut last_err: ai_lib_rust::Error = match builder.execute().await {
        Ok(r) => {
            maybe_emit_telemetry(telemetry, provider_id, model_id, &r, started.elapsed());
            return Ok(r);
        }
        Err(e) => e,
    };
    for attempt in 1..=MAX_RETRIES {
        if !last_err.is_retryable() {
            break;
        }
        if let Some(delay) = last_err.retry_after() {
            tracing::debug!(
                "BYOK retry attempt {} after {:?} (retry_after)",
                attempt,
                delay
            );
            tokio::time::sleep(delay).await;
        } else {
            let backoff = Duration::from_millis(500 * (1 << attempt));
            tracing::debug!(
                "BYOK retry attempt {} after {:?} (exponential backoff)",
                attempt,
                backoff
            );
            tokio::time::sleep(backoff).await;
        }
        let mut builder = client
            .chat()
            .messages(messages.clone())
            .temperature(temperature);
        if let Some(ref t) = tools {
            if !t.is_empty() {
                builder = builder.tools_json(t.clone());
            }
        }
        last_err = match builder.execute().await {
            Ok(r) => {
                maybe_emit_telemetry(telemetry, provider_id, model_id, &r, started.elapsed());
                return Ok(r);
            }
            Err(e) => e,
        };
    }
    Err(last_err)
}

fn maybe_emit_telemetry(
    telemetry: Option<&ByokTelemetryReporter>,
    provider_id: &str,
    model_id: &str,
    response: &ai_lib_rust::client::UnifiedResponse,
    latency: Duration,
) {
    if let Some(telemetry) = telemetry {
        telemetry.emit_byok_success(provider_id, model_id, response.usage.as_ref(), latency);
    }
}
