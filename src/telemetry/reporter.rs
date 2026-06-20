//! Fire-and-forget HTTP reporter for BYOK usage records.
//! BYOK 遥测 HTTP 上报（异步、失败仅记日志）。

use super::record::{parse_token_counts, payload_is_redacted, ByokUsageRecord};
use crate::config::schema::TelemetryConfig;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

const DEFAULT_USER_ID: &str = "velaclaw-local";
const POST_TIMEOUT: Duration = Duration::from_secs(10);

/// Async BYOK usage reporter (VL-EVO-003).
#[derive(Clone)]
pub struct ByokTelemetryReporter {
    endpoint: String,
    user_id: String,
    http: Client,
}

impl ByokTelemetryReporter {
    /// Build reporter when `[telemetry]` is enabled and `endpoint` is set.
    pub fn from_config(config: &TelemetryConfig) -> Option<Arc<Self>> {
        if !config.enabled {
            return None;
        }
        let endpoint = config.endpoint.as_ref()?.trim();
        if endpoint.is_empty() {
            return None;
        }
        let user_id = config
            .user_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_USER_ID.to_string());
        let http = Client::builder().timeout(POST_TIMEOUT).build().ok()?;
        Some(Arc::new(Self {
            endpoint: endpoint.to_string(),
            user_id,
            http,
        }))
    }

    /// Queue an async POST after a successful BYOK chat (non-blocking).
    pub fn emit_byok_success(
        &self,
        provider_id: &str,
        model_id: &str,
        usage: Option<&Value>,
        latency: Duration,
    ) {
        let Some(usage) = usage else {
            tracing::debug!(
                provider = provider_id,
                model = model_id,
                latency_ms = latency.as_millis(),
                "BYOK telemetry skipped: no usage metadata"
            );
            return;
        };

        let (prompt_tokens, completion_tokens, reasoning_tokens) = parse_token_counts(usage);
        let record = ByokUsageRecord::new(
            self.user_id.clone(),
            provider_id,
            model_id,
            prompt_tokens,
            completion_tokens,
            reasoning_tokens,
            0.0,
        );
        let payload = match serde_json::to_value(&record) {
            Ok(v) => v,
            Err(err) => {
                warn!("BYOK telemetry serialize failed: {err}");
                return;
            }
        };
        if !payload_is_redacted(&payload) {
            warn!("BYOK telemetry payload failed redaction check; dropping");
            return;
        }

        let endpoint = self.endpoint.clone();
        let http = self.http.clone();
        tokio::spawn(async move {
            if let Err(err) = http.post(&endpoint).json(&payload).send().await {
                warn!("BYOK telemetry POST failed: {err}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::TelemetryConfig;

    #[test]
    fn disabled_when_telemetry_off() {
        let cfg = TelemetryConfig {
            enabled: false,
            endpoint: Some("http://127.0.0.1:9/usage".into()),
            user_id: None,
        };
        assert!(ByokTelemetryReporter::from_config(&cfg).is_none());
    }

    #[test]
    fn disabled_without_endpoint() {
        let cfg = TelemetryConfig {
            enabled: true,
            endpoint: None,
            user_id: None,
        };
        assert!(ByokTelemetryReporter::from_config(&cfg).is_none());
    }

    #[test]
    fn enabled_with_endpoint() {
        let cfg = TelemetryConfig {
            enabled: true,
            endpoint: Some("http://127.0.0.1:9/v1/usage".into()),
            user_id: Some("alice".into()),
        };
        let reporter = ByokTelemetryReporter::from_config(&cfg).expect("reporter");
        assert_eq!(reporter.user_id, "alice");
        assert_eq!(reporter.endpoint, "http://127.0.0.1:9/v1/usage");
    }
}
