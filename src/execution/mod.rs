//! Execution layer — strategy/execution boundary (VL-ARCH-001 / VL-EVO-001).
//! 执行层：策略层与 ai-lib-rust / prism-core 之间的边界。

mod byok;
mod byok_hygiene;
mod handle;

#[cfg(feature = "prism-router")]
pub mod prism;

pub use byok::{
    execute_chat_with_retry, init_ai_client_sync, resolve_ai_client, split_logical_model_id,
};
pub use byok_hygiene::{
    detected_byok_env_names, diagnose_byok_routing, provider_has_usable_key,
    resolve_byok_logical_model_id, ByokRoutingDiagnosis,
};
pub use handle::{
    hybrid_text_tool_result_history, logical_model_id_from_config,
    nvidia_byok_ai_client_logical_id, nvidia_implied_wire_model_id, ExecutionHandle,
};

/// Build the agent provider stack via [`ExecutionHandle`] (BYOK sole path to ai-lib-rust).
pub fn bootstrap_routed_provider(
    config: &crate::Config,
    options: &crate::providers::ProviderRuntimeOptions,
) -> anyhow::Result<(ExecutionHandle, Box<dyn crate::providers::Provider>)> {
    let execution = ExecutionHandle::from_config(config)?;
    let logical_model = execution.logical_model_id().to_string();
    let provider_name = config
        .default_provider
        .as_deref()
        .unwrap_or(logical_model.as_str());
    let primary_override = Some(execution.provider_adapter()?);
    let provider = crate::providers::create_routed_provider_with_options(
        provider_name,
        config.api_key.as_deref(),
        config.api_url.as_deref(),
        &config.reliability,
        &config.model_routes,
        &logical_model,
        options,
        primary_override,
        config.agent.hint_peer_fallback,
    )?;
    Ok((execution, provider))
}
