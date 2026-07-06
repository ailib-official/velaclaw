//! BYOK execution shim — delegates to `velaclaw-agent-runtime` (VL-ARCH-007).
//! BYOK 执行 shim：委托至 agent-runtime crate。

pub use velaclaw_agent_runtime::{init_ai_client_sync, resolve_ai_client, split_logical_model_id};

use crate::telemetry::ByokTelemetryReporter;
use serde_json::Value;
use std::time::Duration;
use velaclaw_agent_runtime::telemetry::ByokTelemetryHook;

struct ReporterHook<'a>(&'a ByokTelemetryReporter);

impl ByokTelemetryHook for ReporterHook<'_> {
    fn record_success(
        &self,
        provider_id: &str,
        model_id: &str,
        usage: Option<&Value>,
        latency: Duration,
    ) {
        self.0
            .emit_byok_success(provider_id, model_id, usage, latency);
    }
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
    let hook = telemetry.map(ReporterHook);
    velaclaw_agent_runtime::execute_chat_with_retry(
        client,
        provider_id,
        model_id,
        messages,
        temperature,
        tools,
        hook.as_ref().map(|h| h as &dyn ByokTelemetryHook),
    )
    .await
}
