//! Default values shared across onboarding, config, and CLI fallbacks.

/// Canonical `provider/model` id for fresh installs (upstream ai-protocol naming).
/// Uses NVIDIA NIM Nemotron Mini — catalog-listed and broadly entitled; avoids
/// entitlement-gated 340B 404s on many accounts (VL-RT-006; pairs with VL-RT-005 E5c).
/// Must stay in sync with `defaults.toml` (`default_provider` / `default_model`).
pub const DEFAULT_PROTOCOL_MODEL_ID: &str = "nvidia/nemotron-mini-4b-instruct";

/// Human-readable label for [`DEFAULT_PROTOCOL_MODEL_ID`] in onboarding and CLI lists.
pub const DEFAULT_PROTOCOL_MODEL_LABEL: &str = "NVIDIA Nemotron Mini 4B Instruct (default)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_defaults_toml_matches_protocol_model_id() {
        let raw = include_str!("defaults.toml");
        let template: toml::Value =
            toml::from_str(raw).expect("defaults.toml must parse as valid TOML");
        assert_eq!(
            template.get("default_provider").and_then(|v| v.as_str()),
            Some(DEFAULT_PROTOCOL_MODEL_ID),
            "default_provider must match DEFAULT_PROTOCOL_MODEL_ID"
        );
        assert_eq!(
            template.get("default_model").and_then(|v| v.as_str()),
            Some(DEFAULT_PROTOCOL_MODEL_ID),
            "default_model must match DEFAULT_PROTOCOL_MODEL_ID"
        );
    }
}
