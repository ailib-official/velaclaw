//! Prism-routed provider — embedded `prism-core` router + libcurl proxy (VL-EVO-002).
//! Prism 路由提供者：内嵌 prism-core 路由与 libcurl 转发。

use crate::providers::traits::{ChatMessage, Provider, ProviderCapabilities, ToolsPayload};
use crate::telemetry::ByokTelemetryReporter;
use async_trait::async_trait;
use prism_core::config::{ConfigProvider, EnvConfig, ModelEntry, ProviderEntry};
use prism_core::proxy;
use prism_core::router::{ErrorClass, FallbackRouter, RequestResult};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Instant;

const MAX_ROUTER_ATTEMPTS: u32 = 3;

/// Provider backed by in-process `prism-core` fallback routing (not BYOK `AiClient`).
pub struct PrismBackedProvider {
    router: Arc<FallbackRouter>,
    env: Arc<EnvConfig>,
    route_model_id: String,
    #[allow(dead_code)]
    telemetry: Option<Arc<ByokTelemetryReporter>>,
}

impl PrismBackedProvider {
    pub fn new(
        router: Arc<FallbackRouter>,
        env: Arc<EnvConfig>,
        logical_model_id: String,
        telemetry: Option<Arc<ByokTelemetryReporter>>,
    ) -> anyhow::Result<Self> {
        let route_model_id = crate::execution::prism::route_model_id(&logical_model_id).to_string();
        Ok(Self {
            router,
            env,
            route_model_id,
            telemetry,
        })
    }

    fn to_openai_messages(messages: &[ChatMessage]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect()
    }

    async fn execute_chat(&self, messages: Vec<Value>, temperature: f64) -> anyhow::Result<String> {
        let router = Arc::clone(&self.router);
        let env = Arc::clone(&self.env);
        let route_model = self.route_model_id.clone();
        tokio::task::spawn_blocking(move || {
            execute_openai_chat_blocking(&router, &env, &route_model, messages, temperature)
        })
        .await
        .map_err(|_| anyhow::anyhow!("prism chat worker panicked"))?
    }
}

fn execute_openai_chat_blocking(
    router: &FallbackRouter,
    env: &EnvConfig,
    route_model: &str,
    messages: Vec<Value>,
    temperature: f64,
) -> anyhow::Result<String> {
    let body = json!({
        "model": route_model,
        "messages": messages,
        "temperature": temperature,
    });
    let headers = Map::new();

    for _ in 0..MAX_ROUTER_ATTEMPTS {
        let Some(decision) = router.route(route_model) else {
            anyhow::bail!("no prism router decision for model `{route_model}`");
        };
        let url = prism_core::config::openai_chat_completions_url(&decision.provider_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "prism provider `{}` has no OpenAI-compatible chat URL",
                    decision.provider_id
                )
            })?;

        let started = Instant::now();
        let proxy_cfg = RoutedProxyConfig {
            env: env.clone(),
            provider_id: decision.provider_id.clone(),
            api_key: decision.api_key.clone(),
        };
        let resp = proxy::curl_proxy(url, &headers, &body, &proxy_cfg);
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let error_class = ErrorClass::from_status(resp.status);

        router.report_result(&RequestResult {
            provider_id: decision.provider_id.clone(),
            key_id: decision.key_id.clone(),
            status_code: resp.status,
            latency_ms,
            error_class: error_class.clone(),
        });

        match error_class {
            ErrorClass::Success => return extract_openai_content(&resp.body),
            ErrorClass::NonRetryable => {
                anyhow::bail!(
                    "prism provider `{}` error {}: {}",
                    decision.provider_id,
                    resp.status,
                    resp.body
                );
            }
            ErrorClass::Retryable | ErrorClass::RateLimited => continue,
        }
    }

    anyhow::bail!("prism router exhausted retries for `{route_model}`")
}

fn extract_openai_content(body: &Value) -> anyhow::Result<String> {
    body.pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("missing choices[0].message.content in response: {body}"))
}

#[derive(Clone)]
struct RoutedProxyConfig {
    env: EnvConfig,
    provider_id: String,
    api_key: String,
}

impl ConfigProvider for RoutedProxyConfig {
    fn providers(&self) -> &[ProviderEntry] {
        self.env.providers()
    }

    fn resolve_api_key(&self, provider_id: &str) -> Option<String> {
        if provider_id == self.provider_id {
            Some(self.api_key.clone())
        } else {
            self.env.resolve_api_key(provider_id)
        }
    }

    fn available_models(&self) -> Vec<&ModelEntry> {
        self.env.available_models()
    }
}

#[async_trait]
impl Provider for PrismBackedProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: false,
            vision: false,
        }
    }

    fn convert_tools(&self, tools: &[crate::tools::ToolSpec]) -> ToolsPayload {
        ToolsPayload::PromptGuided {
            instructions: crate::providers::traits::build_tool_instructions_text(tools),
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(json!({ "role": "system", "content": sys }));
        }
        messages.push(json!({ "role": "user", "content": message }));
        self.execute_chat(messages, temperature).await
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        self.execute_chat(Self::to_openai_messages(messages), temperature)
            .await
    }
}
