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
        let logical_model_id = logical_model_id_from_config(config);
        let routing = config.routing.clone();
        let telemetry = ByokTelemetryReporter::from_config(&config.telemetry);

        let backend = match config.routing.provider_mode {
            ProviderRoutingMode::Byok => {
                ExecutionBackend::Byok(init_ai_client_sync(&logical_model_id)?)
            }
            ProviderRoutingMode::Prism => {
                #[cfg(feature = "prism-router")]
                {
                    ExecutionBackend::Prism(PrismRouterHandle::from_config(config)?)
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
pub fn logical_model_id_from_config(config: &Config) -> String {
    let model = config
        .default_model
        .as_deref()
        .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID);
    if model.contains('/') {
        return model.to_string();
    }

    let provider = config
        .default_provider
        .as_deref()
        .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID);
    if provider.contains('/') {
        return provider.to_string();
    }

    format!("{provider}/{model}")
}

fn init_ai_client_sync(model_id: &str) -> anyhow::Result<Arc<ai_lib_rust::AiClient>> {
    let client = if tokio::runtime::Handle::try_current().is_ok() {
        let model_for_thread = model_id.to_string();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                providers::protocol_adapter::resolve_ai_client(&model_for_thread).await
            })
        })
        .join()
        .map_err(|_| anyhow::anyhow!("execution handle initialization thread panicked"))??
    } else {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(providers::protocol_adapter::resolve_ai_client(model_id))?
    };

    Ok(Arc::new(client))
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
        config.default_model = Some("deepseek/deepseek-chat".into());
        assert_eq!(
            logical_model_id_from_config(&config),
            "deepseek/deepseek-chat"
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
