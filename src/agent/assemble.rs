//! Canonical Config → runtime stack assembly (VL-REVIEW2-A0 / GOV-007).
//!
//! CLI (`loop_::run` / `process_message`), Web (`Agent::from_config`), and Channel
//! startup share this entry for provider / memory / security / tools / dispatcher.
//! Transport adapters (stdin approval, ApprovalHub, channel listeners, peripheral
//! tool merge) stay outside.

use crate::agent::dispatcher::ToolDispatcher;
#[cfg(not(feature = "ai-protocol"))]
use crate::agent::dispatcher::{NativeToolDispatcher, XmlToolDispatcher};
#[cfg(not(feature = "ai-protocol"))]
use crate::config::DEFAULT_PROTOCOL_MODEL_ID;
use crate::config::{bootstrap_runtime, BootstrapOptions, Config, RuntimeBootstrap};
use crate::providers::Provider;
use anyhow::{Context, Result};

/// Shared assembled stack used by Agent, CLI loop, and Channel hosts.
pub struct AssembledRuntime {
    pub boot: RuntimeBootstrap,
    pub provider: Box<dyn Provider>,
    pub model_name: String,
    pub tool_dispatcher: Box<dyn ToolDispatcher>,
    /// When true, tool results use `[Tool results]` user text (Hybrid manifests).
    pub text_tool_result_history: bool,
    #[cfg(feature = "ai-protocol")]
    pub execution: Option<crate::execution::ExecutionHandle>,
}

/// Single production entry: Config → security/memory/tools/provider/dispatcher.
///
/// `options.with_embedding_routes` selects Agent-style memory (true) vs
/// CLI/Channel storage-only memory (false) — same factory, not a second path.
pub fn assemble_runtime(config: &Config, options: BootstrapOptions) -> Result<AssembledRuntime> {
    let boot = bootstrap_runtime(config, options)?;
    let provider_runtime_options = boot.provider_runtime_options.clone();

    #[cfg(feature = "ai-protocol")]
    {
        let (exec_handle, provider) =
            crate::execution::bootstrap_routed_provider(config, &provider_runtime_options)?;
        let model_name = exec_handle.logical_model_id().to_string();
        let tool_calling_policy = exec_handle.tool_calling_policy();
        let text_tool_result_history =
            tool_calling_policy.native_strategy == ai_lib_rust::NativeStrategy::Hybrid;
        let workspace_policy =
            crate::config::discover_and_load(config).context("load workspace agent-policy.yaml")?;
        let workspace_dispatcher = workspace_policy.as_ref().and_then(|p| p.tool_dispatcher());
        let effective = crate::config::EffectivePolicy::resolve(
            config.agent.tool_dispatcher.as_str(),
            workspace_dispatcher,
            None,
            tool_calling_policy,
        );
        let tool_dispatcher = effective.build_dispatcher(provider.as_ref());
        Ok(AssembledRuntime {
            boot,
            provider,
            model_name,
            tool_dispatcher,
            text_tool_result_history,
            execution: Some(exec_handle),
        })
    }

    #[cfg(not(feature = "ai-protocol"))]
    {
        let provider_name = config
            .default_provider
            .as_deref()
            .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID);
        let model_name = config
            .default_model
            .as_deref()
            .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID)
            .to_string();
        let provider = crate::providers::create_routed_provider_with_options(
            provider_name,
            config.api_key.as_deref(),
            config.api_url.as_deref(),
            &config.reliability,
            &config.model_routes,
            &model_name,
            &provider_runtime_options,
            None,
            config.agent.hint_peer_fallback,
        )?;
        let workspace_policy =
            crate::config::discover_and_load(config).context("load workspace agent-policy.yaml")?;
        let workspace_dispatcher = workspace_policy.as_ref().and_then(|p| p.tool_dispatcher());
        let choice = crate::config::merge_tool_dispatcher(
            config.agent.tool_dispatcher.as_str(),
            workspace_dispatcher,
            None,
        );
        let tool_dispatcher: Box<dyn ToolDispatcher> = match choice.as_str() {
            "native" => Box::new(NativeToolDispatcher::default()),
            "xml" => Box::new(XmlToolDispatcher::default()),
            _ if provider.supports_native_tools() => Box::new(NativeToolDispatcher::default()),
            _ => Box::new(XmlToolDispatcher::default()),
        };
        let text_tool_result_history = !tool_dispatcher.should_send_tool_specs();
        Ok(AssembledRuntime {
            boot,
            provider,
            model_name,
            tool_dispatcher,
            text_tool_result_history,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn assemble_runtime_shares_bootstrap_for_agent_and_cli_options() {
        let dir = TempDir::new().expect("tempdir");
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        config.config_path = dir.path().join("config.toml");
        config.memory.backend = "none".into();

        // Shared lower layer must succeed without credentials.
        let agent_boot = bootstrap_runtime(
            &config,
            BootstrapOptions {
                with_embedding_routes: true,
            },
        )
        .expect("agent bootstrap");
        let cli_boot = bootstrap_runtime(
            &config,
            BootstrapOptions {
                with_embedding_routes: false,
            },
        )
        .expect("cli bootstrap");
        assert_eq!(agent_boot.runtime.name(), cli_boot.runtime.name());
        assert!(!agent_boot.observer.name().is_empty());
        assert!(!cli_boot.observer.name().is_empty());
    }
}
