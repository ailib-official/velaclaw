//! Shared runtime bootstrap for Agent / Gateway / Channels (VL-REVIEW-005).
//! Lower layer for [`crate::agent::assemble::assemble_runtime`] (VL-REVIEW2-A0).

use crate::config::Config;
use crate::memory::{self, Memory};
use crate::observability::{self, Observer};
use crate::providers::ProviderRuntimeOptions;
use crate::runtime::{self, RuntimeAdapter};
use crate::security::PolicyHandle;
use crate::tools::{self, Tool};
use anyhow::Result;
use std::sync::Arc;

/// Core runtime handles shared by Agent and Gateway startup paths.
pub struct RuntimeBootstrap {
    pub security: PolicyHandle,
    pub runtime: Arc<dyn RuntimeAdapter>,
    pub memory: Arc<dyn Memory>,
    pub observer: Arc<dyn Observer>,
    pub tools: Vec<Box<dyn Tool>>,
    /// Attach point for gateway [`crate::approval::HumanInputHub`].
    pub human_input_attach: tools::HumanInputAttach,
    pub provider_runtime_options: ProviderRuntimeOptions,
}

/// Options controlling memory construction for [`bootstrap_runtime`].
#[derive(Clone, Copy, Debug, Default)]
pub struct BootstrapOptions {
    /// When true, include embedding routes (Agent path). Gateway uses false.
    pub with_embedding_routes: bool,
}

impl RuntimeBootstrap {
    /// Build shared security / runtime / memory / tools / observer from config.
    pub fn from_config(config: &Config, options: BootstrapOptions) -> Result<Self> {
        bootstrap_runtime(config, options)
    }
}

/// Construct the shared runtime stack used by Agent and Gateway.
pub fn bootstrap_runtime(config: &Config, options: BootstrapOptions) -> Result<RuntimeBootstrap> {
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let runtime: Arc<dyn RuntimeAdapter> = Arc::from(runtime::create_runtime(&config.runtime)?);
    let security = PolicyHandle::from_workspace_config(config)?;

    let memory: Arc<dyn Memory> = if options.with_embedding_routes {
        Arc::from(memory::create_memory_with_storage_and_routes(
            &config.memory,
            &config.embedding_routes,
            Some(&config.storage.provider.config),
            &config.workspace_dir,
            config.api_key.as_deref(),
        )?)
    } else {
        Arc::from(memory::create_memory_with_storage(
            &config.memory,
            Some(&config.storage.provider.config),
            &config.workspace_dir,
            config.api_key.as_deref(),
        )?)
    };

    let (composio_key, composio_entity_id) = if config.composio.enabled {
        (
            config.composio.api_key.as_deref(),
            Some(config.composio.entity_id.as_str()),
        )
    } else {
        (None, None)
    };

    let (tools, human_input_attach) = tools::all_tools_with_runtime(
        Arc::new(config.clone()),
        &security,
        Arc::clone(&runtime),
        Arc::clone(&memory),
        composio_key,
        composio_entity_id,
        &config.browser,
        &config.http_request,
        &config.workspace_dir,
        &config.agents,
        config.api_key.as_deref(),
        config,
    );

    let provider_runtime_options = ProviderRuntimeOptions {
        auth_profile_override: None,
        velaclaw_dir: config.config_path.parent().map(std::path::PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        reasoning_enabled: config.runtime.reasoning_enabled,
    };

    Ok(RuntimeBootstrap {
        security,
        runtime,
        memory,
        observer,
        tools,
        human_input_attach,
        provider_runtime_options,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use tempfile::TempDir;

    #[test]
    fn bootstrap_runtime_builds_core_handles() {
        let dir = TempDir::new().expect("tempdir");
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        config.config_path = dir.path().join("config.toml");
        config.memory.backend = "markdown".into();

        let boot = bootstrap_runtime(
            &config,
            BootstrapOptions {
                with_embedding_routes: true,
            },
        )
        .expect("bootstrap");
        assert!(!boot.tools.is_empty());
        assert!(!boot.runtime.name().is_empty());
        let _ = boot.security.autonomy();
    }
}
