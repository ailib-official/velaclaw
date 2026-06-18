//! Execution layer — strategy/execution boundary (VL-ARCH-001 / VL-EVO-001).
//! 执行层：策略层与 ai-lib-rust / prism-core 之间的边界。

mod handle;

pub use handle::ExecutionHandle;

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
    )?;
    Ok((execution, provider))
}
