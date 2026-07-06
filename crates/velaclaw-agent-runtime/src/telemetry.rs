//! BYOK telemetry hook — implemented by velaclaw main crate (VL-ARCH-007).

use serde_json::Value;
use std::time::Duration;

pub trait ByokTelemetryHook: Send + Sync {
    fn record_success(
        &self,
        provider_id: &str,
        model_id: &str,
        usage: Option<&Value>,
        latency: Duration,
    );
}
