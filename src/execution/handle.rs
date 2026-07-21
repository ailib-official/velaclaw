//! In-process execution handle — BYOK (`ai-lib-rust`) and Prism router paths (VL-EVO-001/002).
//! 进程内执行句柄：BYOK 与内嵌 Prism 路由。

use crate::config::{
    Config, ExecutionRoutingConfig, ProviderRoutingMode, DEFAULT_PROTOCOL_MODEL_ID,
};
use crate::providers::{self, Provider};
use crate::telemetry::ByokTelemetryReporter;
use std::sync::Arc;

#[cfg(feature = "prism-router")]
use super::prism::PrismRouterHandle;

/// Backend for the execution layer.
pub enum ExecutionBackend {
    Byok(Arc<ai_lib_rust::AiClient>),
    #[cfg(feature = "prism-router")]
    Prism(PrismRouterHandle),
}

/// Unified execution entry from strategy layer to ai-lib-rust / prism-core.
pub struct ExecutionHandle {
    backend: ExecutionBackend,
    logical_model_id: String,
    routing: ExecutionRoutingConfig,
    telemetry: Option<Arc<ByokTelemetryReporter>>,
}

impl ExecutionHandle {
    /// Build an execution handle from top-level config (sync; may block on init).
    pub fn from_config(config: &Config) -> anyhow::Result<Self> {
        let routing = config.routing.clone();
        let telemetry = ByokTelemetryReporter::from_config(&config.telemetry);

        let (backend, logical_model_id) = match config.routing.provider_mode {
            ProviderRoutingMode::Byok => {
                // VL-RT-003: host-side hygiene before AiClient init (prefer keyed
                // provider or actionable fail — do not silent-404 on nvidia default).
                let logical_model_id = super::resolve_byok_logical_model_id(config)?;
                let backend =
                    ExecutionBackend::Byok(super::byok::init_ai_client_sync(&logical_model_id)?);
                (backend, logical_model_id)
            }
            ProviderRoutingMode::Prism => {
                #[cfg(feature = "prism-router")]
                {
                    let logical_model_id = logical_model_id_from_config(config);
                    let backend = ExecutionBackend::Prism(PrismRouterHandle::from_config(config)?);
                    (backend, logical_model_id)
                }
                #[cfg(not(feature = "prism-router"))]
                {
                    anyhow::bail!(
                        "routing.provider_mode = \"prism\" requires the prism-router Cargo feature"
                    );
                }
            }
        };

        Ok(Self {
            backend,
            logical_model_id,
            routing,
            telemetry,
        })
    }

    /// Logical `provider/model` id for the configured execution path.
    pub fn logical_model_id(&self) -> &str {
        &self.logical_model_id
    }

    pub fn routing(&self) -> &ExecutionRoutingConfig {
        &self.routing
    }

    /// Whether this handle uses BYOK direct `AiClient` execution.
    pub fn is_byok(&self) -> bool {
        matches!(self.backend, ExecutionBackend::Byok(_))
    }

    /// Whether this handle uses embedded prism-core routing.
    pub fn is_prism_routed(&self) -> bool {
        #[cfg(feature = "prism-router")]
        {
            matches!(self.backend, ExecutionBackend::Prism(_))
        }
        #[cfg(not(feature = "prism-router"))]
        {
            false
        }
    }

    /// Shared `AiClient` for BYOK execution.
    pub fn byok_client(&self) -> Option<Arc<ai_lib_rust::AiClient>> {
        match &self.backend {
            ExecutionBackend::Byok(client) => Some(Arc::clone(client)),
            #[cfg(feature = "prism-router")]
            ExecutionBackend::Prism(_) => None,
        }
    }

    /// Whether native tool calling is fully reliable for the current provider/model
    /// (VL-TTC-001: `native.reliability == full` from manifest `tool_calling`).
    pub fn native_tool_calling_is_reliable(&self) -> bool {
        matches!(
            self.tool_calling_policy().native_strategy,
            ai_lib_rust::NativeStrategy::Full
        )
    }

    /// Manifest-driven tool calling policy (parser + native strategy).
    pub fn tool_calling_policy(&self) -> ai_lib_rust::ToolCallingPolicy {
        match &self.backend {
            ExecutionBackend::Byok(client) => {
                ai_lib_rust::ToolCallingPolicy::from_tool_calling(client.manifest.tool_calling())
            }
            #[cfg(feature = "prism-router")]
            ExecutionBackend::Prism(_) => ai_lib_rust::ToolCallingPolicy::from_tool_calling(None),
        }
    }

    /// Provider manifest `tool_calling` block (VL-TTC-002/003 parser wiring).
    pub fn manifest_tool_calling(&self) -> Option<&serde_json::Value> {
        match &self.backend {
            ExecutionBackend::Byok(client) => client.manifest.tool_calling(),
            #[cfg(feature = "prism-router")]
            ExecutionBackend::Prism(_) => None,
        }
    }

    /// Trait adapter for tool-loop compatibility.
    pub fn provider_adapter(&self) -> anyhow::Result<Box<dyn Provider>> {
        match &self.backend {
            ExecutionBackend::Byok(client) => Ok(Box::new(
                providers::protocol_adapter::ProtocolBackedProvider::from_client(
                    Arc::clone(client),
                    &self.logical_model_id,
                    self.telemetry.clone(),
                )?,
            )),
            #[cfg(feature = "prism-router")]
            ExecutionBackend::Prism(prism) => Ok(Box::new(prism.provider(self.telemetry.clone())?)),
        }
    }
}

/// Resolve the logical model id used to construct `AiClient` or prism router.
///
/// Composition rules (VL-RT-004):
/// - Bare `-p nvidia` + `--model meta/llama-…` → `nvidia/meta/llama-…`
///   (vendor-qualified NIM ids must not drop the CLI provider).
/// - Bare `-p nvidia` + `--model nvidia/nemotron-…` → `nvidia/nemotron-…`
///   (do not double-prefix when `--model` already starts with the provider).
/// - `--model provider/model` alone (no bare provider pin) keeps the model id.
pub fn logical_model_id_from_config(config: &Config) -> String {
    let model = config
        .default_model
        .as_deref()
        .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID)
        .trim();
    let provider = config
        .default_provider
        .as_deref()
        .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID)
        .trim();

    if model.contains('/') {
        let model_provider = model.split_once('/').map(|(p, _)| p).unwrap_or(model);
        // Bare provider pin whose first segment differs from `--model`'s first
        // segment → treat model as vendor-qualified under that provider (E5b).
        if !provider.contains('/')
            && !provider.is_empty()
            && !model_provider.eq_ignore_ascii_case(provider)
        {
            return format!("{provider}/{model}");
        }
        return model.to_string();
    }

    if provider.contains('/') {
        return provider.to_string();
    }

    format!("{provider}/{model}")
}

/// Whether tool-loop history should use `[Tool results]` text (Hybrid manifest strategy).
#[cfg(feature = "ai-protocol")]
pub fn hybrid_text_tool_result_history(logical_model_id: &str) -> bool {
    super::init_ai_client_sync(logical_model_id)
        .ok()
        .is_some_and(|client| {
            ai_lib_rust::ToolCallingPolicy::from_tool_calling(client.manifest.tool_calling())
                .native_strategy
                == ai_lib_rust::NativeStrategy::Hybrid
        })
}

#[cfg(not(feature = "ai-protocol"))]
pub fn hybrid_text_tool_result_history(_logical_model_id: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn logical_model_combines_provider_and_model() {
        let mut config = Config::default();
        config.default_provider = Some("openai".into());
        config.default_model = Some("gpt-4o".into());
        assert_eq!(logical_model_id_from_config(&config), "openai/gpt-4o");
    }

    #[test]
    fn logical_model_uses_slashed_default_model() {
        let mut config = Config::default();
        config.default_model = Some("deepseek/deepseek-v4-flash".into());
        assert_eq!(
            logical_model_id_from_config(&config),
            "deepseek/deepseek-v4-flash"
        );
    }

    #[test]
    fn logical_model_prefers_provider_segment_when_not_slashed() {
        let mut config = Config::default();
        // Correct shape after CLI `-p deepseek --model deepseek-v4-flash` / config fix.
        config.default_provider = Some("deepseek".into());
        config.default_model = Some("deepseek-v4-flash".into());
        assert_eq!(
            logical_model_id_from_config(&config),
            "deepseek/deepseek-v4-flash"
        );
    }

    #[test]
    fn logical_model_keeps_slashed_provider_as_full_id() {
        // Misconfigured `default_provider = "deepseek/deepseek-chat"` previously
        // short-circuited model selection; document current resolution contract.
        let mut config = Config::default();
        config.default_provider = Some("deepseek/deepseek-chat".into());
        config.default_model = Some("deepseek-v4-flash".into());
        assert_eq!(
            logical_model_id_from_config(&config),
            "deepseek/deepseek-chat"
        );
    }

    #[test]
    fn logical_model_bare_provider_plus_vendor_qualified_model() {
        // VL-RT-004 / E5b: `-p nvidia --model meta/llama-3.1-8b-instruct`
        let mut config = Config::default();
        config.default_provider = Some("nvidia".into());
        config.default_model = Some("meta/llama-3.1-8b-instruct".into());
        assert_eq!(
            logical_model_id_from_config(&config),
            "nvidia/meta/llama-3.1-8b-instruct"
        );
    }

    #[test]
    fn logical_model_bare_provider_plus_already_prefixed_model() {
        let mut config = Config::default();
        config.default_provider = Some("nvidia".into());
        config.default_model = Some("nvidia/nemotron-mini-4b-instruct".into());
        assert_eq!(
            logical_model_id_from_config(&config),
            "nvidia/nemotron-mini-4b-instruct"
        );
    }

    #[test]
    fn logical_model_vendor_qualified_without_bare_provider_pin() {
        let mut config = Config::default();
        // Slashed default_provider must not re-prefix a full `--model` id.
        config.default_provider = Some(crate::config::DEFAULT_PROTOCOL_MODEL_ID.into());
        config.default_model = Some("nvidia/nemotron-mini-4b-instruct".into());
        assert_eq!(
            logical_model_id_from_config(&config),
            "nvidia/nemotron-mini-4b-instruct"
        );
    }

    #[test]
    fn byok_mode_selected_by_default() {
        let config = Config::default();
        assert_eq!(config.routing.provider_mode, ProviderRoutingMode::Byok);
    }

    #[cfg(feature = "prism-router")]
    #[test]
    fn prism_mode_requires_prism_api_keys() {
        let mut config = Config::default();
        config.routing.provider_mode = ProviderRoutingMode::Prism;
        config.default_model = Some("llama-3.1-8b-instant".into());
        match ExecutionHandle::from_config(&config) {
            Err(e) => assert!(e.to_string().contains("PRISM_")),
            Ok(_) => panic!("expected prism mode to fail without PRISM_* API keys"),
        }
    }

    #[cfg(feature = "prism-router")]
    #[test]
    fn byok_handle_exposes_ai_client() {
        // Without AI_PROTOCOL_DIR this may fail; skip when env not configured.
        if std::env::var("AI_PROTOCOL_DIR").is_err() {
            return;
        }
        let config = Config::default();
        if let Ok(handle) = ExecutionHandle::from_config(&config) {
            assert!(handle.is_byok());
            assert!(handle.byok_client().is_some());
        }
    }
}
