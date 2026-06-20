//! 提供者子系统，管理不同 AI 模型的连接与负载均衡。
//! Provider subsystem for model inference backends.
//!
//! # Protocol provider ids
//!
//! Provider construction is protocol-first: use `provider/model` logical ids
//! backed by ai-protocol manifests. Legacy string keys and built-in vendor HTTP
//! adapters were removed in ZS-ML-015; see **`docs/migration-legacy-to-protocol.md`**.
//!
//! The subsystem supports resilient multi-provider configurations through the
//! [`ReliableProvider`](reliable::ReliableProvider) wrapper, which handles fallback
//! chains and automatic retry. Model routing across providers is available via
//! [`create_routed_provider`].
//! 提供者模块负责模型连接、切换与容错策略。
//!
//! # Extension
//!
//! To add a new provider, add or update an ai-protocol manifest instead of
//! wiring vendor-specific HTTP code in VelaClaw.

pub mod openai_codex;
pub mod reliable;
pub mod router;
pub mod traits;

#[cfg(feature = "ai-protocol")]
pub mod protocol_adapter;

#[cfg(feature = "prism-router")]
pub mod prism_adapter;

#[allow(unused_imports)]
pub use traits::{
    ChatMessage, ChatRequest, ChatResponse, ConversationMessage, Provider, ProviderCapabilityError,
    ToolCall, ToolResultMessage,
};

use reliable::ReliableProvider;
use std::path::PathBuf;

const MAX_API_ERROR_CHARS: usize = 200;
const PROVIDER_MODE_ENV: &str = "VELACLAW_PROVIDER_MODE";

pub(crate) fn is_minimax_intl_alias(name: &str) -> bool {
    matches!(
        name,
        "minimax"
            | "minimax-intl"
            | "minimax-io"
            | "minimax-global"
            | "minimax-oauth"
            | "minimax-portal"
            | "minimax-oauth-global"
            | "minimax-portal-global"
    )
}

pub(crate) fn is_minimax_cn_alias(name: &str) -> bool {
    matches!(
        name,
        "minimax-cn" | "minimaxi" | "minimax-oauth-cn" | "minimax-portal-cn"
    )
}

pub(crate) fn is_minimax_alias(name: &str) -> bool {
    is_minimax_intl_alias(name) || is_minimax_cn_alias(name)
}

pub(crate) fn is_glm_global_alias(name: &str) -> bool {
    matches!(name, "glm" | "zhipu" | "glm-global" | "zhipu-global")
}

pub(crate) fn is_glm_cn_alias(name: &str) -> bool {
    matches!(name, "glm-cn" | "zhipu-cn" | "bigmodel")
}

pub(crate) fn is_glm_alias(name: &str) -> bool {
    is_glm_global_alias(name) || is_glm_cn_alias(name)
}

pub(crate) fn is_moonshot_intl_alias(name: &str) -> bool {
    matches!(
        name,
        "moonshot-intl" | "moonshot-global" | "kimi-intl" | "kimi-global"
    )
}

pub(crate) fn is_moonshot_cn_alias(name: &str) -> bool {
    matches!(name, "moonshot" | "kimi" | "moonshot-cn" | "kimi-cn")
}

pub(crate) fn is_moonshot_alias(name: &str) -> bool {
    is_moonshot_intl_alias(name) || is_moonshot_cn_alias(name)
}

pub(crate) fn is_qwen_cn_alias(name: &str) -> bool {
    matches!(name, "qwen" | "dashscope" | "qwen-cn" | "dashscope-cn")
}

pub(crate) fn is_qwen_intl_alias(name: &str) -> bool {
    matches!(
        name,
        "qwen-intl" | "dashscope-intl" | "qwen-international" | "dashscope-international"
    )
}

pub(crate) fn is_qwen_us_alias(name: &str) -> bool {
    matches!(name, "qwen-us" | "dashscope-us")
}

pub(crate) fn is_qwen_oauth_alias(name: &str) -> bool {
    matches!(name, "qwen-code" | "qwen-oauth" | "qwen_oauth")
}

pub(crate) fn is_qwen_alias(name: &str) -> bool {
    is_qwen_cn_alias(name)
        || is_qwen_intl_alias(name)
        || is_qwen_us_alias(name)
        || is_qwen_oauth_alias(name)
}

pub(crate) fn is_zai_global_alias(name: &str) -> bool {
    matches!(name, "zai" | "z.ai" | "zai-global" | "z.ai-global")
}

pub(crate) fn is_zai_cn_alias(name: &str) -> bool {
    matches!(name, "zai-cn" | "z.ai-cn")
}

pub(crate) fn is_zai_alias(name: &str) -> bool {
    is_zai_global_alias(name) || is_zai_cn_alias(name)
}

pub(crate) fn is_qianfan_alias(name: &str) -> bool {
    matches!(name, "qianfan" | "baidu")
}

pub(crate) fn is_doubao_alias(name: &str) -> bool {
    matches!(name, "doubao" | "volcengine" | "ark" | "doubao-cn")
}

pub(crate) fn canonical_china_provider_name(name: &str) -> Option<&'static str> {
    if is_qwen_alias(name) {
        Some("qwen")
    } else if is_glm_alias(name) {
        Some("glm")
    } else if is_moonshot_alias(name) {
        Some("moonshot")
    } else if is_minimax_alias(name) {
        Some("minimax")
    } else if is_zai_alias(name) {
        Some("zai")
    } else if is_qianfan_alias(name) {
        Some("qianfan")
    } else if is_doubao_alias(name) {
        Some("doubao")
    } else {
        None
    }
}

fn read_non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn protocol_only_mode_enabled() -> bool {
    read_non_empty_env(PROVIDER_MODE_ENV)
        .map(|mode| {
            mode.eq_ignore_ascii_case("protocol-only") || mode.eq_ignore_ascii_case("protocol")
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct ProviderRuntimeOptions {
    pub auth_profile_override: Option<String>,
    pub velaclaw_dir: Option<PathBuf>,
    pub secrets_encrypt: bool,
    pub reasoning_enabled: Option<bool>,
}

impl Default for ProviderRuntimeOptions {
    fn default() -> Self {
        Self {
            auth_profile_override: None,
            velaclaw_dir: None,
            secrets_encrypt: true,
            reasoning_enabled: None,
        }
    }
}

fn is_secret_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')
}

fn token_end(input: &str, from: usize) -> usize {
    let mut end = from;
    for (i, c) in input[from..].char_indices() {
        if is_secret_char(c) {
            end = from + i + c.len_utf8();
        } else {
            break;
        }
    }
    end
}

/// Scrub known secret-like token prefixes from provider error strings.
pub fn scrub_secret_patterns(input: &str) -> String {
    const PREFIXES: [&str; 7] = [
        "sk-",
        "xoxb-",
        "xoxp-",
        "ghp_",
        "gho_",
        "ghu_",
        "github_pat_",
    ];

    let mut scrubbed = input.to_string();

    for prefix in PREFIXES {
        let mut search_from = 0;
        while let Some(rel) = scrubbed[search_from..].find(prefix) {
            let start = search_from + rel;
            let content_start = start + prefix.len();
            let end = token_end(&scrubbed, content_start);

            if end == content_start {
                search_from = content_start;
                continue;
            }

            scrubbed.replace_range(start..end, "[REDACTED]");
            search_from = start + "[REDACTED]".len();
        }
    }

    scrubbed
}

/// Sanitize API error text by scrubbing secrets and truncating length.
pub fn sanitize_api_error(input: &str) -> String {
    let scrubbed = scrub_secret_patterns(input);

    if scrubbed.chars().count() <= MAX_API_ERROR_CHARS {
        return scrubbed;
    }

    let mut end = MAX_API_ERROR_CHARS;
    while end > 0 && !scrubbed.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}...", &scrubbed[..end])
}

/// Build a sanitized provider error from a failed HTTP response.
pub async fn api_error(provider: &str, response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read provider error body>".to_string());
    let sanitized = sanitize_api_error(&body);
    anyhow::anyhow!("{provider} API error ({status}): {sanitized}")
}

/// Parse protocol provider syntax from either:
/// - `protocol:provider/model`
/// - `provider/model` (shorthand)
pub(crate) fn parse_protocol_provider_model(name: &str) -> Option<(&str, &str)> {
    let trimmed = name.trim();
    let rest = if let Some(stripped) = trimmed.strip_prefix("protocol:") {
        stripped.trim()
    } else {
        if trimmed.starts_with("custom:") || trimmed.starts_with("anthropic-custom:") {
            return None;
        }
        if trimmed.contains("://") {
            return None;
        }
        trimmed
    };

    let (provider_id, model_id) = rest.split_once('/')?;
    let provider_id = provider_id.trim();
    let model_id = model_id.trim();
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some((provider_id, model_id))
}

fn legacy_provider_removed_error(name: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "Legacy provider key `{name}` was removed in ZS-ML-015. Use `provider/model` syntax \
         (for example `{example}`) with `AI_PROTOCOL_DIR` pointing at an ai-protocol checkout. \
         For custom endpoints, create an ai-protocol provider manifest instead of `custom:` or \
         `anthropic-custom:` URL syntax. See `docs/migration-legacy-to-protocol.md`.",
        example = crate::config::DEFAULT_PROTOCOL_MODEL_ID,
        name = name,
    )
}

/// Factory: create the right provider from config (without custom URL).
pub fn create_provider(name: &str, api_key: Option<&str>) -> anyhow::Result<Box<dyn Provider>> {
    create_provider_with_options(name, api_key, &ProviderRuntimeOptions::default())
}

/// Factory: create provider with runtime options.
pub fn create_provider_with_options(
    name: &str,
    api_key: Option<&str>,
    options: &ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    match name.trim() {
        "openai-codex" | "openai_codex" | "codex" => {
            Ok(Box::new(openai_codex::OpenAiCodexProvider::new(options)))
        }
        _ => create_provider_with_url_and_options(name, api_key, None, options),
    }
}

/// Factory: create provider with optional base URL.
///
/// `api_url` is retained for config compatibility but ignored by protocol providers; endpoint
/// selection belongs in ai-protocol manifests after ZS-ML-015.
pub fn create_provider_with_url(
    name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
) -> anyhow::Result<Box<dyn Provider>> {
    create_provider_with_url_and_options(name, api_key, api_url, &ProviderRuntimeOptions::default())
}

fn create_provider_with_url_and_options(
    name: &str,
    _api_key: Option<&str>,
    api_url: Option<&str>,
    options: &ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        anyhow::bail!(
            "Provider name cannot be empty. Use provider/model syntax, for example `{example}`.",
            example = crate::config::DEFAULT_PROTOCOL_MODEL_ID,
        );
    }

    #[cfg(feature = "ai-protocol")]
    if let Some((provider_id, model_id)) = parse_protocol_provider_model(trimmed) {
        return Ok(Box::new(
            protocol_adapter::ProtocolBackedProvider::from_logical_model(provider_id, model_id)?,
        ));
    }

    #[cfg(not(feature = "ai-protocol"))]
    if parse_protocol_provider_model(trimmed).is_some() {
        anyhow::bail!(
            "Protocol provider requires --features ai-protocol. Build with: cargo build --features ai-protocol"
        );
    }

    let _ = (api_url, options);
    if protocol_only_mode_enabled() {
        anyhow::bail!(
            "Provider mode is protocol-only via {PROVIDER_MODE_ENV}. Use provider/model syntax (e.g. {example}).",
            example = crate::config::DEFAULT_PROTOCOL_MODEL_ID,
        );
    }

    Err(legacy_provider_removed_error(trimmed))
}

/// Create provider chain with retry and fallback behavior.
pub fn create_resilient_provider(
    primary_name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
) -> anyhow::Result<Box<dyn Provider>> {
    create_resilient_provider_with_options(
        primary_name,
        api_key,
        api_url,
        reliability,
        &ProviderRuntimeOptions::default(),
        None,
    )
}

/// Create provider chain with retry/fallback behavior and auth runtime options.
pub fn create_resilient_provider_with_options(
    primary_name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
    options: &ProviderRuntimeOptions,
    primary_override: Option<Box<dyn Provider>>,
) -> anyhow::Result<Box<dyn Provider>> {
    let mut providers: Vec<(String, Box<dyn Provider>)> = Vec::new();

    let primary_provider = match primary_override {
        Some(provider) => provider,
        None => create_provider_with_url_and_options(primary_name, api_key, api_url, options)?,
    };
    providers.push((primary_name.to_string(), primary_provider));

    for fallback in &reliability.fallback_providers {
        if fallback == primary_name || providers.iter().any(|(name, _)| name == fallback) {
            continue;
        }

        match create_provider_with_options(fallback, None, options) {
            Ok(provider) => providers.push((fallback.clone(), provider)),
            Err(error) => {
                tracing::warn!(
                    fallback_provider = fallback,
                    error = %sanitize_api_error(&error.to_string()),
                    "Ignoring invalid fallback provider during initialization"
                );
            }
        }
    }

    let reliable = ReliableProvider::new(
        providers,
        reliability.provider_retries,
        reliability.provider_backoff_ms,
    )
    .with_api_keys(reliability.api_keys.clone())
    .with_model_fallbacks(reliability.model_fallbacks.clone());

    Ok(Box::new(reliable))
}

/// Create a RouterProvider if model routes are configured, otherwise return a
/// standard resilient provider. The router wraps individual providers per route,
/// each with its own retry/fallback chain.
pub fn create_routed_provider(
    primary_name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
    model_routes: &[crate::config::ModelRouteConfig],
    default_model: &str,
) -> anyhow::Result<Box<dyn Provider>> {
    create_routed_provider_with_options(
        primary_name,
        api_key,
        api_url,
        reliability,
        model_routes,
        default_model,
        &ProviderRuntimeOptions::default(),
        None,
    )
}

/// Create a routed provider using explicit runtime options.
pub fn create_routed_provider_with_options(
    primary_name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
    model_routes: &[crate::config::ModelRouteConfig],
    default_model: &str,
    options: &ProviderRuntimeOptions,
    primary_override: Option<Box<dyn Provider>>,
) -> anyhow::Result<Box<dyn Provider>> {
    if model_routes.is_empty() {
        return create_resilient_provider_with_options(
            primary_name,
            api_key,
            api_url,
            reliability,
            options,
            primary_override,
        );
    }

    let mut needed: Vec<String> = vec![primary_name.to_string()];
    for route in model_routes {
        if !needed.iter().any(|name| name == &route.provider) {
            needed.push(route.provider.clone());
        }
    }

    let mut providers: Vec<(String, Box<dyn Provider>)> = Vec::new();
    let mut primary_override = primary_override;
    for name in &needed {
        let routed_credential = model_routes
            .iter()
            .find(|route| &route.provider == name)
            .and_then(|route| {
                route.api_key.as_ref().and_then(|raw_key| {
                    let trimmed_key = raw_key.trim();
                    (!trimmed_key.is_empty()).then_some(trimmed_key)
                })
            });
        let key = routed_credential.or(api_key);
        let url = if name == primary_name { api_url } else { None };
        let override_for_name = if name == primary_name {
            primary_override.take()
        } else {
            None
        };
        match create_resilient_provider_with_options(
            name,
            key,
            url,
            reliability,
            options,
            override_for_name,
        ) {
            Ok(provider) => providers.push((name.clone(), provider)),
            Err(e) => {
                if name == primary_name {
                    return Err(e);
                }
                tracing::warn!(
                    provider = name.as_str(),
                    error = %sanitize_api_error(&e.to_string()),
                    "Ignoring routed provider that failed to initialize"
                );
            }
        }
    }

    let routes: Vec<(String, router::Route)> = model_routes
        .iter()
        .map(|route| {
            (
                route.hint.clone(),
                router::Route {
                    provider_name: route.provider.clone(),
                    model: route.model.clone(),
                },
            )
        })
        .collect();

    Ok(Box::new(router::RouterProvider::new(
        providers,
        routes,
        default_model.to_string(),
    )))
}

/// Information about a supported provider display row.
pub struct ProviderInfo {
    /// Canonical provider key shown in config examples.
    pub name: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Alternative names accepted in config.
    pub aliases: &'static [&'static str],
    /// Whether the provider runs locally (no API key required).
    pub local: bool,
}

/// Return protocol-first provider examples for `velaclaw providers list`.
pub fn list_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            name: crate::config::DEFAULT_PROTOCOL_MODEL_ID,
            display_name: "OpenAI via ai-protocol",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "anthropic/claude-3-5-sonnet",
            display_name: "Anthropic via ai-protocol",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "google/gemini-1.5-pro",
            display_name: "Google Gemini via ai-protocol",
            aliases: &["gemini/gemini-1.5-pro"],
            local: false,
        },
        ProviderInfo {
            name: "deepseek/deepseek-chat",
            display_name: "DeepSeek via ai-protocol",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "qwen/qwen-max",
            display_name: "Qwen via ai-protocol",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "openai-codex",
            display_name: "OpenAI Codex (OAuth)",
            aliases: &["openai_codex", "codex"],
            local: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_protocol_provider_model_accepts_prefixed_and_shorthand() {
        assert_eq!(
            parse_protocol_provider_model("protocol:openai/gpt-5.2"),
            Some(("openai", "gpt-5.2"))
        );
        assert_eq!(
            parse_protocol_provider_model("openai/gpt-5.2"),
            Some(("openai", "gpt-5.2"))
        );
        assert!(parse_protocol_provider_model("custom:https://example.com").is_none());
        assert!(parse_protocol_provider_model("anthropic-custom:https://example.com").is_none());
        assert!(parse_protocol_provider_model("https://example.com").is_none());
    }

    #[cfg(not(feature = "ai-protocol"))]
    #[test]
    fn factory_protocol_syntax_requires_feature_when_disabled() {
        let prefixed = create_provider("protocol:openai/gpt-5.2", Some("test-key"));
        let shorthand = create_provider("openai/gpt-5.2", Some("test-key"));
        assert!(prefixed.is_err());
        assert!(shorthand.is_err());
        let prefixed_msg = prefixed.err().unwrap().to_string();
        let shorthand_msg = shorthand.err().unwrap().to_string();
        assert!(prefixed_msg.contains("requires --features ai-protocol"));
        assert!(shorthand_msg.contains("requires --features ai-protocol"));
    }

    #[test]
    fn legacy_string_key_errors_with_migration_hint() {
        let result = create_provider("openrouter", Some("test-key"));
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("removed in ZS-ML-015"));
        assert!(msg.contains("provider/model"));
        assert!(msg.contains("docs/migration-legacy-to-protocol.md"));
    }

    #[test]
    fn custom_url_key_errors_with_manifest_hint() {
        let result = create_provider("custom:https://example.com/v1", Some("test-key"));
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("provider manifest"));
        assert!(msg.contains("custom:"));
    }

    #[test]
    fn listed_provider_examples_are_unique() {
        let providers = list_providers();
        let mut names = std::collections::HashSet::new();
        let mut aliases = std::collections::HashSet::new();
        for provider in providers {
            assert!(names.insert(provider.name));
            for alias in provider.aliases {
                assert_ne!(*alias, provider.name);
                assert!(aliases.insert(alias));
            }
        }
    }

    #[test]
    fn canonical_china_provider_name_maps_regional_aliases() {
        assert_eq!(canonical_china_provider_name("moonshot"), Some("moonshot"));
        assert_eq!(canonical_china_provider_name("kimi-intl"), Some("moonshot"));
        assert_eq!(canonical_china_provider_name("glm"), Some("glm"));
        assert_eq!(canonical_china_provider_name("zhipu-cn"), Some("glm"));
        assert_eq!(canonical_china_provider_name("minimax"), Some("minimax"));
        assert_eq!(canonical_china_provider_name("minimax-cn"), Some("minimax"));
        assert_eq!(canonical_china_provider_name("qwen"), Some("qwen"));
        assert_eq!(canonical_china_provider_name("dashscope-us"), Some("qwen"));
        assert_eq!(canonical_china_provider_name("qwen-code"), Some("qwen"));
        assert_eq!(canonical_china_provider_name("zai"), Some("zai"));
        assert_eq!(canonical_china_provider_name("z.ai-cn"), Some("zai"));
        assert_eq!(canonical_china_provider_name("qianfan"), Some("qianfan"));
        assert_eq!(canonical_china_provider_name("baidu"), Some("qianfan"));
        assert_eq!(canonical_china_provider_name("doubao"), Some("doubao"));
        assert_eq!(canonical_china_provider_name("volcengine"), Some("doubao"));
        assert_eq!(canonical_china_provider_name("openai"), None);
    }

    #[test]
    fn sanitize_scrubs_multiple_prefixes() {
        let input = "keys sk-abcdef xoxb-12345 xoxp-67890 ghp_secret";
        let out = sanitize_api_error(input);
        assert!(!out.contains("sk-abcdef"));
        assert!(!out.contains("xoxb-12345"));
        assert!(!out.contains("xoxp-67890"));
        assert!(!out.contains("ghp_secret"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn sanitize_truncates_long_error() {
        let long = "a".repeat(400);
        let result = sanitize_api_error(&long);
        assert!(result.len() <= 203);
        assert!(result.ends_with("..."));
    }
}
