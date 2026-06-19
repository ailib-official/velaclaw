//! UsageRecord-shaped payload aligned with prism-core `usage::UsageRecord`.
//! 与 prism-core UsageRecord 字段对齐的遥测载荷。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Keys that must never appear in outbound BYOK telemetry (VL-ARCH-001 D5).
pub const FORBIDDEN_PAYLOAD_KEYS: &[&str] = &[
    "api_key",
    "api-key",
    "authorization",
    "bearer",
    "prompt",
    "content",
    "messages",
    "secret",
    "password",
];

/// Prism-compatible usage record (see `prism-core::usage::UsageRecord`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ByokUsageRecord {
    pub id: String,
    pub user_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
    pub cost_usd: f64,
    pub timestamp: i64,
}

impl ByokUsageRecord {
    pub fn new(
        user_id: impl Into<String>,
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        prompt_tokens: u32,
        completion_tokens: u32,
        reasoning_tokens: Option<u32>,
        cost_usd: f64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.into(),
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            prompt_tokens,
            completion_tokens,
            reasoning_tokens,
            cost_usd,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

/// Parse token counts from ai-lib `UnifiedResponse.usage` JSON.
pub fn parse_token_counts(usage: &Value) -> (u32, u32, Option<u32>) {
    let prompt = usage_u64_field(usage, &["prompt_tokens", "input_tokens"]);
    let completion = usage_u64_field(usage, &["completion_tokens", "output_tokens"]);
    let reasoning = usage
        .get("reasoning_tokens")
        .and_then(|v| v.as_u64())
        .map(usage_u64_to_u32);
    (prompt, completion, reasoning)
}

fn usage_u64_field(usage: &Value, keys: &[&str]) -> u32 {
    keys.iter()
        .find_map(|key| usage.get(*key))
        .and_then(|v| v.as_u64())
        .map(usage_u64_to_u32)
        .unwrap_or(0)
}

fn usage_u64_to_u32(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Returns true when serialized JSON contains no forbidden secret/prompt fields.
pub fn payload_is_redacted(value: &Value) -> bool {
    !json_contains_forbidden_key(value)
}

fn json_contains_forbidden_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let lower = key.to_ascii_lowercase();
                if FORBIDDEN_PAYLOAD_KEYS
                    .iter()
                    .any(|forbidden| lower.contains(forbidden))
                {
                    return true;
                }
                if json_contains_forbidden_key(child) {
                    return true;
                }
            }
            false
        }
        Value::Array(items) => items.iter().any(json_contains_forbidden_key),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_serializes_prism_usage_shape() {
        let record = ByokUsageRecord::new(
            "velaclaw-local",
            "deepseek",
            "deepseek-chat",
            120,
            45,
            None,
            0.0,
        );
        let json = serde_json::to_value(&record).unwrap();
        assert!(json.get("id").is_some());
        assert_eq!(json["user_id"], "velaclaw-local");
        assert_eq!(json["provider_id"], "deepseek");
        assert_eq!(json["model_id"], "deepseek-chat");
        assert_eq!(json["prompt_tokens"], 120);
        assert_eq!(json["completion_tokens"], 45);
        assert!(json.get("reasoning_tokens").is_none());
        assert_eq!(json["cost_usd"], 0.0);
        assert!(json.get("timestamp").is_some());
        assert!(payload_is_redacted(&json));
    }

    #[test]
    fn parse_openai_style_usage() {
        let usage = serde_json::json!({
            "prompt_tokens": 10,
            "completion_tokens": 20,
            "total_tokens": 30
        });
        assert_eq!(parse_token_counts(&usage), (10, 20, None));
    }

    #[test]
    fn parse_input_output_style_usage() {
        let usage = serde_json::json!({
            "input_tokens": 5,
            "output_tokens": 7,
            "reasoning_tokens": 2
        });
        assert_eq!(parse_token_counts(&usage), (5, 7, Some(2)));
    }

    #[test]
    fn forbidden_keys_detected_in_nested_json() {
        let bad = serde_json::json!({ "meta": { "api_key": "sk-secret" } });
        assert!(!payload_is_redacted(&bad));
        let good = serde_json::json!({ "provider_id": "openai", "prompt_tokens": 1 });
        assert!(payload_is_redacted(&good));
    }
}
