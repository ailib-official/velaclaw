//! BYOK telemetry hook — implemented by velaclaw main crate (VL-ARCH-007).

use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct UsageSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub trait ByokTelemetryHook: Send + Sync {
    fn record_success(
        &self,
        provider_id: &str,
        model_id: &str,
        usage: Option<UsageSnapshot>,
        latency: Duration,
    );
}
