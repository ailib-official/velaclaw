//! In-process execution handle — sole BYOK path to `ai-lib-rust` (VL-EVO-001).
//! 进程内执行句柄：BYOK 场景下 agent 循环到 ai-lib-rust 的唯一入口。

use crate::config::{
    Config, ExecutionRoutingConfig, ProviderRoutingMode, DEFAULT_PROTOCOL_MODEL_ID,
};
use crate::providers::{self, Provider};
use std::sync::Arc;

/// Backend for the execution layer. EVO-001 implements BYOK only.
pub enum ExecutionBackend {
    Byok(Arc<ai_lib_rust::AiClient>),
}

/// Unified execution entry from strategy layer to ai-lib-rust / prism-core.
pub struct ExecutionHandle {
    backend: ExecutionBackend,
    logical_model_id: String,
    routing: ExecutionRoutingConfig,
}

impl ExecutionHandle {
    /// Build a BYOK handle from top-level config (sync; blocks on `AiClient::new`).
    pub fn from_config(config: &Config) -> anyhow::Result<Self> {
        if config.routing.provider_mode == ProviderRoutingMode::Prism {
            anyhow::bail!(
                "routing.provider_mode = \"prism\" requires embedded prism-core (VL-EVO-002); \
                 use \"byok\" (default) for direct provider access"
            );
        }

        let logical_model_id = logical_model_id_from_config(config);
        let client = init_ai_client_sync(&logical_model_id)?;

        Ok(Self {
            backend: ExecutionBackend::Byok(client),
            logical_model_id,
            routing: config.routing.clone(),
        })
    }

    /// Logical `provider/model` id bound on the `AiClient` (no per-request override).
    pub fn logical_model_id(&self) -> &str {
        &self.logical_model_id
    }

    pub fn routing(&self) -> &ExecutionRoutingConfig {
        &self.routing
    }

    /// Shared `AiClient` for BYOK execution.
    pub fn byok_client(&self) -> Option<Arc<ai_lib_rust::AiClient>> {
        match &self.backend {
            ExecutionBackend::Byok(client) => Some(Arc::clone(client)),
        }
    }

    /// Trait adapter for tool-loop compatibility; chat uses the bound `AiClient` model only.
    pub fn provider_adapter(&self) -> anyhow::Result<Box<dyn Provider>> {
        let client = self
            .byok_client()
            .ok_or_else(|| anyhow::anyhow!("BYOK client not available"))?;
        Ok(Box::new(
            providers::protocol_adapter::ProtocolBackedProvider::from_client(
                client,
                &self.logical_model_id,
            )?,
        ))
    }
}

/// Resolve the logical model id used to construct `AiClient`.
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
    fn prism_mode_rejected_at_handle_construction() {
        let mut config = Config::default();
        config.routing.provider_mode = ProviderRoutingMode::Prism;
        let err = ExecutionHandle::from_config(&config).unwrap_err();
        assert!(err.to_string().contains("VL-EVO-002"));
    }
}
