//! BYOK execution helpers — AiClient init and telemetry (VL-ARCH-007).
//! BYOK 执行辅助：客户端初始化与遥测。
//!
//! [GOV-007] Micro-retries around `chat().execute()` were removed. Transport /
//! provider retries live inside `ai-lib-rust` (`execute_with_retry` /
//! PolicyEngine). App-layer logic here only initializes the client and records
//! telemetry — use ReliableProvider for provider/model failover, not a second
//! attempt loop on the same call.

use crate::telemetry::ByokTelemetryHook;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

pub async fn execute_chat_with_retry(
    client: &ai_lib_rust::AiClient,
    provider_id: &str,
    model_id: &str,
    messages: Vec<ai_lib_rust::Message>,
    temperature: f64,
    tools: Option<Vec<serde_json::Value>>,
    telemetry: Option<&dyn ByokTelemetryHook>,
) -> Result<ai_lib_rust::client::UnifiedResponse, ai_lib_rust::Error> {
    let started = Instant::now();
    let mut builder = client.chat().messages(messages).temperature(temperature);
    if let Some(ref t) = tools {
        if !t.is_empty() {
            builder = builder.tools_json(t.clone());
        }
    }
    // Single execute — library PolicyEngine owns retries ([GOV-007] / VELA-004).
    let response = builder.execute().await?;
    maybe_emit_telemetry(
        telemetry,
        provider_id,
        model_id,
        &response,
        started.elapsed(),
    );
    Ok(response)
}

fn maybe_emit_telemetry(
    telemetry: Option<&dyn ByokTelemetryHook>,
    provider_id: &str,
    model_id: &str,
    response: &ai_lib_rust::client::UnifiedResponse,
    latency: Duration,
) {
    if let Some(telemetry) = telemetry {
        telemetry.record_success(provider_id, model_id, response.usage.as_ref(), latency);
    }
}
