//! Host-side BYOK default model hygiene (VL-RT-003).
//! 宿主侧 BYOK 默认模型卫生：无 key 的默认 provider 不静默打到 404。
//!
//! Strategy lives here (before [`super::ExecutionHandle`] BYOK init). Actual
//! secret resolution remains ai-lib PT-074 / `AiClient`.

use crate::config::Config;
use std::collections::BTreeSet;

/// Resolve the logical `provider/model` id for BYOK execution.
///
/// When the configured provider has no usable env credential, prefer another
/// provider that does (and warn). If none are available, return an actionable
/// error instead of letting `AiClient` hit a remote 404.
pub fn resolve_byok_logical_model_id(config: &Config) -> anyhow::Result<String> {
    match diagnose_byok_routing(config).effective {
        Ok(id) => Ok(id),
        Err(msg) => Err(anyhow::anyhow!(msg)),
    }
}

/// Operator-facing BYOK routing diagnosis (env names only; never secret values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByokRoutingDiagnosis {
    pub configured: String,
    /// Effective logical model id, or fail-closed message when none is usable.
    pub effective: Result<String, String>,
}

/// Diagnose BYOK configured vs effective model (same rules as runtime init).
///
/// On remap, emits the same `tracing::warn!` as [`resolve_byok_logical_model_id`].
pub fn diagnose_byok_routing(config: &Config) -> ByokRoutingDiagnosis {
    let configured = super::logical_model_id_from_config(config);
    let configured_provider = provider_segment(&configured);

    if provider_has_usable_key(configured_provider) {
        return ByokRoutingDiagnosis {
            configured: configured.clone(),
            effective: Ok(configured),
        };
    }

    if let Some(remapped) = first_keyed_logical_model(config, configured_provider) {
        let hint_env = primary_env_hint(configured_provider);
        tracing::warn!(
            configured = %configured,
            remapped = %remapped,
            missing_env_hint = %hint_env,
            "BYOK default provider has no usable API key; remapping to a keyed provider \
             (set {hint_env} or default_model to pin the original)"
        );
        return ByokRoutingDiagnosis {
            configured,
            effective: Ok(remapped),
        };
    }

    ByokRoutingDiagnosis {
        configured: configured.clone(),
        effective: Err(missing_key_error(&configured, configured_provider, config).to_string()),
    }
}

/// Detected provider credential env var *names* (never values).
pub fn detected_byok_env_names(config: &Config) -> Vec<String> {
    detected_provider_env_names(config).into_iter().collect()
}

fn missing_key_error(configured: &str, provider: &str, config: &Config) -> anyhow::Error {
    let needed = env_candidates(provider);
    let needed_msg = if needed.is_empty() {
        primary_env_hint(provider)
    } else {
        needed.join(" / ")
    };
    let detected = detected_provider_env_names(config);
    let detected_msg = if detected.is_empty() {
        "none detected for known providers".to_string()
    } else {
        detected.into_iter().collect::<Vec<_>>().join(", ")
    };
    anyhow::anyhow!(
        "BYOK cannot start: configured model `{configured}` needs a credential for provider \
         `{provider}` (try {needed_msg}), but no usable key was found.\n\
         Detected provider env keys: {detected_msg}.\n\
         Fix: export a key for `{provider}`, or set `default_model` / `default_provider` \
         to a provider you have keyed (see docs/providers-reference.md)."
    )
}

fn provider_segment(logical_model_id: &str) -> &str {
    logical_model_id
        .split_once('/')
        .map(|(provider, _)| provider)
        .unwrap_or(logical_model_id)
}

/// Whether this provider is callable on this host (keyless local or non-empty env key).
///
/// Used by BYOK hygiene and CR-CAP-004 reachable capability views. Never returns
/// secret values — presence only.
pub fn provider_has_usable_key(provider: &str) -> bool {
    if provider_supports_keyless(provider) {
        return true;
    }
    provider_has_env_key(provider)
}

fn provider_has_env_key(provider: &str) -> bool {
    let curated = env_candidates(provider);
    if !curated.is_empty() {
        return curated.iter().any(|name| env_nonempty(name));
    }
    ai_lib_rust::credentials::conventional_envs(provider)
        .iter()
        .any(|name| env_nonempty(name))
}

fn provider_supports_keyless(provider: &str) -> bool {
    matches!(
        normalize_provider(provider).as_str(),
        "ollama" | "llamacpp" | "lmstudio"
    )
}

fn first_keyed_logical_model(config: &Config, skip_provider: &str) -> Option<String> {
    let mut seen = BTreeSet::new();
    for candidate in candidate_providers(config) {
        let normalized = normalize_provider(&candidate);
        if normalized == normalize_provider(skip_provider) {
            continue;
        }
        if !seen.insert(normalized.clone()) {
            continue;
        }
        // Remap targets must have a real env key — do not silently fall through to
        // keyless local providers when cloud keys are missing.
        if !provider_has_env_key(&candidate) {
            continue;
        }
        return Some(default_logical_model_for_provider(&candidate));
    }
    None
}

fn candidate_providers(config: &Config) -> Vec<String> {
    let mut out = Vec::new();
    for raw in &config.reliability.fallback_providers {
        let provider = provider_segment(raw.trim());
        if !provider.is_empty() {
            out.push(provider.to_string());
        }
    }
    for provider in SCAN_PRIORITY {
        out.push((*provider).to_string());
    }
    out
}

/// Prefer reliability.fallback_providers, then this stable BYOK scan order.
const SCAN_PRIORITY: &[&str] = &[
    "openai",
    "anthropic",
    "deepseek",
    "groq",
    "nvidia",
    "mistral",
    "gemini",
    "xai",
    "openrouter",
    "moonshot",
    "glm",
    "qwen",
    "together",
    "fireworks",
    "cohere",
    "perplexity",
    "ollama",
    "llamacpp",
];

fn default_logical_model_for_provider(provider: &str) -> String {
    let normalized = normalize_provider(provider);
    let model = match normalized.as_str() {
        "openai" => "gpt-4o-mini",
        "anthropic" => "claude-sonnet-4-20250514",
        "deepseek" => "deepseek-chat",
        "groq" => "llama-3.1-8b-instant",
        "nvidia" => "nemotron-4-340b-instruct",
        "mistral" => "mistral-small-latest",
        "gemini" => "gemini-2.0-flash",
        "xai" => "grok-2-latest",
        "openrouter" => "openai/gpt-4o-mini",
        "moonshot" => "moonshot-v1-8k",
        "glm" => "glm-4-flash",
        "qwen" => "qwen-plus",
        "together" => "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo",
        "fireworks" => "accounts/fireworks/models/llama-v3p1-8b-instruct",
        "cohere" => "command-r",
        "perplexity" => "sonar",
        "ollama" => "llama3.2",
        "llamacpp" => "local-model",
        other => return format!("{other}/default"),
    };
    format!("{normalized}/{model}")
}

fn normalize_provider(provider: &str) -> String {
    let lower = provider.trim().to_ascii_lowercase();
    match lower.as_str() {
        "nvidia-nim" | "build.nvidia.com" => "nvidia".into(),
        "google" | "google-gemini" => "gemini".into(),
        "grok" => "xai".into(),
        "together-ai" => "together".into(),
        "fireworks-ai" => "fireworks".into(),
        "llama.cpp" => "llamacpp".into(),
        "lm-studio" => "lmstudio".into(),
        "kimi" => "moonshot".into(),
        "zhipu" => "glm".into(),
        "dashscope" => "qwen".into(),
        other => other.to_string(),
    }
}

fn env_candidates(provider: &str) -> Vec<&'static str> {
    let normalized = normalize_provider(provider);
    match normalized.as_str() {
        "openai" => vec!["OPENAI_API_KEY"],
        "anthropic" => vec!["ANTHROPIC_API_KEY", "ANTHROPIC_OAUTH_TOKEN"],
        "deepseek" => vec!["DEEPSEEK_API_KEY"],
        "groq" => vec!["GROQ_API_KEY"],
        "nvidia" => vec!["NVIDIA_API_KEY"],
        "mistral" => vec!["MISTRAL_API_KEY"],
        "gemini" => vec!["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        "xai" => vec!["XAI_API_KEY"],
        "openrouter" => vec!["OPENROUTER_API_KEY"],
        "moonshot" => vec!["MOONSHOT_API_KEY"],
        "glm" => vec!["GLM_API_KEY"],
        "qwen" => vec!["DASHSCOPE_API_KEY", "QWEN_OAUTH_TOKEN"],
        "together" => vec!["TOGETHER_API_KEY"],
        "fireworks" => vec!["FIREWORKS_API_KEY"],
        "cohere" => vec!["COHERE_API_KEY"],
        "perplexity" => vec!["PERPLEXITY_API_KEY"],
        "ollama" => vec!["OLLAMA_API_KEY"],
        "llamacpp" => vec!["LLAMACPP_API_KEY"],
        _ => vec![],
    }
}

fn primary_env_hint(provider: &str) -> String {
    let candidates = env_candidates(provider);
    if let Some(first) = candidates.first() {
        return (*first).to_string();
    }
    let normalized = normalize_provider(provider)
        .to_uppercase()
        .replace('-', "_");
    format!("{normalized}_API_KEY")
}

fn env_nonempty(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn detected_provider_env_names(config: &Config) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for provider in SCAN_PRIORITY {
        for env_name in env_candidates(provider) {
            if env_nonempty(env_name) {
                found.insert((*env_name).to_string());
            }
        }
        if env_candidates(provider).is_empty() {
            for name in ai_lib_rust::credentials::conventional_envs(provider) {
                if env_nonempty(&name) {
                    found.insert(name);
                }
            }
        }
    }
    if config
        .api_key
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        // Noted for operator diagnostics only — BYOK AiClient does not consume it.
        found.insert("config.api_key(not used by BYOK AiClient)".into());
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DEFAULT_PROTOCOL_MODEL_ID};
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(key).ok();
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn clear_common_keys() -> Vec<EnvGuard> {
        [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "DEEPSEEK_API_KEY",
            "GROQ_API_KEY",
            "NVIDIA_API_KEY",
            "MISTRAL_API_KEY",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "XAI_API_KEY",
            "OPENROUTER_API_KEY",
        ]
        .into_iter()
        .map(|k| EnvGuard::set(k, None))
        .collect()
    }

    #[test]
    fn byok_hygiene_keeps_configured_when_provider_key_present() {
        let _lock = env_lock().lock().unwrap();
        let _cleared = clear_common_keys();
        let _nvidia = EnvGuard::set("NVIDIA_API_KEY", Some("nv-test-key"));
        let config = Config::default();
        let resolved = resolve_byok_logical_model_id(&config).expect("resolve");
        assert_eq!(resolved, DEFAULT_PROTOCOL_MODEL_ID);
    }

    #[test]
    fn byok_hygiene_remaps_default_nvidia_to_openai_when_only_openai_key() {
        let _lock = env_lock().lock().unwrap();
        let _cleared = clear_common_keys();
        let _openai = EnvGuard::set("OPENAI_API_KEY", Some("sk-test-openai"));
        let config = Config::default();
        let resolved = resolve_byok_logical_model_id(&config).expect("resolve");
        assert!(
            resolved.starts_with("openai/"),
            "expected openai remap, got {resolved}"
        );
    }

    #[test]
    fn byok_hygiene_fails_actionably_when_no_keys() {
        let _lock = env_lock().lock().unwrap();
        let _cleared = clear_common_keys();
        let config = Config::default();
        let err = resolve_byok_logical_model_id(&config).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("BYOK cannot start"), "{msg}");
        assert!(
            msg.contains("NVIDIA_API_KEY") || msg.contains("nvidia"),
            "{msg}"
        );
    }

    #[test]
    fn byok_hygiene_keeps_explicit_openai_when_keyed() {
        let _lock = env_lock().lock().unwrap();
        let _cleared = clear_common_keys();
        let _openai = EnvGuard::set("OPENAI_API_KEY", Some("sk-test-openai"));
        let mut config = Config::default();
        config.default_model = Some("openai/gpt-4o".into());
        config.default_provider = Some("openai".into());
        let resolved = resolve_byok_logical_model_id(&config).expect("resolve");
        assert_eq!(resolved, "openai/gpt-4o");
    }

    #[test]
    fn byok_hygiene_prefers_fallback_providers_order() {
        let _lock = env_lock().lock().unwrap();
        let _cleared = clear_common_keys();
        let _openai = EnvGuard::set("OPENAI_API_KEY", Some("sk-openai"));
        let _groq = EnvGuard::set("GROQ_API_KEY", Some("gsk-groq"));
        let mut config = Config::default();
        config.reliability.fallback_providers = vec!["groq".into(), "openai".into()];
        let resolved = resolve_byok_logical_model_id(&config).expect("resolve");
        assert!(
            resolved.starts_with("groq/"),
            "expected groq first via fallback_providers, got {resolved}"
        );
    }
}
