//! Embedded prism-core router handle (VL-EVO-002).
//! 内嵌 prism-core 路由句柄。

use crate::config::Config;
use crate::providers::prism_adapter::PrismBackedProvider;
use crate::telemetry::ByokTelemetryReporter;
use prism_core::config::{ConfigProvider, EnvConfig};
use prism_core::key_pool::{KeyEntry, KeyPool, KeyState};
use prism_core::router::FallbackRouter;
use std::collections::HashMap;
use std::sync::Arc;

/// In-process prism-core router + env config for Prism-routed execution.
pub struct PrismRouterHandle {
    router: Arc<FallbackRouter>,
    env: Arc<EnvConfig>,
    logical_model_id: String,
}

impl PrismRouterHandle {
    pub fn from_config(config: &Config) -> anyhow::Result<Self> {
        let logical_model_id = crate::execution::logical_model_id_from_config(config);
        let env = Arc::new(EnvConfig::from_env());
        let router = build_router(&env, &logical_model_id)?;
        Ok(Self {
            router: Arc::new(router),
            env,
            logical_model_id,
        })
    }

    pub fn logical_model_id(&self) -> &str {
        &self.logical_model_id
    }

    pub fn router(&self) -> &FallbackRouter {
        &self.router
    }

    pub fn env(&self) -> &EnvConfig {
        &self.env
    }

    pub fn provider(
        &self,
        telemetry: Option<Arc<ByokTelemetryReporter>>,
    ) -> anyhow::Result<PrismBackedProvider> {
        PrismBackedProvider::new(
            Arc::clone(&self.router),
            Arc::clone(&self.env),
            self.logical_model_id.clone(),
            telemetry,
        )
    }
}

/// Model id passed to [`FallbackRouter::route`] (strip `provider/` prefix).
pub fn route_model_id(logical_model_id: &str) -> &str {
    logical_model_id
        .rsplit_once('/')
        .map(|(_, model)| model)
        .unwrap_or(logical_model_id)
}

fn build_router(env: &EnvConfig, logical_model_id: &str) -> anyhow::Result<FallbackRouter> {
    let route_model = route_model_id(logical_model_id).to_string();
    let mut key_entries = Vec::new();
    let mut base_urls = HashMap::new();
    let mut provider_ids = Vec::new();

    for provider in env.providers() {
        let Some(key) = env.resolve_api_key(&provider.id) else {
            continue;
        };
        provider_ids.push(provider.id.clone());
        key_entries.push(KeyEntry {
            id: format!("{}-velaclaw", provider.id),
            provider_id: provider.id.clone(),
            key,
            state: KeyState::Active,
        });
        let host = provider
            .base_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');
        base_urls.insert(provider.id.clone(), format!("https://{host}"));
    }

    if key_entries.is_empty() {
        anyhow::bail!(
            "routing.provider_mode = \"prism\" requires at least one PRISM_*_API_KEY env var \
             (e.g. PRISM_GROQ_API_KEY, PRISM_DEEPSEEK_API_KEY)"
        );
    }

    let pool = KeyPool::new(key_entries);
    let fallback_order = vec![(route_model, provider_ids)];
    Ok(FallbackRouter::new(fallback_order, base_urls, pool))
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::key_pool::{KeyEntry, KeyPool, KeyState};
    use prism_core::router::FallbackRouter;
    use std::collections::HashMap;

    fn test_router() -> FallbackRouter {
        let keys = vec![KeyEntry {
            id: "groq-test".into(),
            provider_id: "groq".into(),
            key: "test-key".into(),
            state: KeyState::Active,
        }];
        let mut urls = HashMap::new();
        urls.insert("groq".into(), "https://api.groq.com".into());
        FallbackRouter::new(
            vec![("llama-3.1-8b-instant".into(), vec!["groq".into()])],
            urls,
            KeyPool::new(keys),
        )
    }

    #[test]
    fn route_model_id_strips_provider_prefix() {
        assert_eq!(
            route_model_id("groq/llama-3.1-8b-instant"),
            "llama-3.1-8b-instant"
        );
        assert_eq!(
            route_model_id("llama-3.1-8b-instant"),
            "llama-3.1-8b-instant"
        );
    }

    #[test]
    fn embedded_router_returns_route_decision() {
        let router = test_router();
        let decision = router
            .route("llama-3.1-8b-instant")
            .expect("route decision");
        assert_eq!(decision.provider_id, "groq");
    }
}
