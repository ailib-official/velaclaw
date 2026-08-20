//! Onboard wizard orchestration (VL-REVIEW2-A1).
//!
//! Entry points stay here; step bodies live in [`steps`], model refresh in [`models`].

use crate::config::schema::{
    DingTalkConfig, IrcConfig, LarkReceiveMode, LinqConfig, QQConfig, StreamMode, WhatsAppConfig,
};
use crate::config::{
    AutonomyConfig, BrowserConfig, ChannelApprovalMode, ChannelsConfig, ComposioConfig, Config,
    DiscordConfig, HeartbeatConfig, IMessageConfig, LarkConfig, MatrixConfig, MemoryConfig,
    ObservabilityConfig, RuntimeConfig, SecretsConfig, SlackConfig, StorageConfig, TelegramConfig,
    WebhookConfig, DEFAULT_PROTOCOL_MODEL_ID, DEFAULT_PROTOCOL_MODEL_LABEL,
};
use crate::hardware::{self, HardwareConfig};
use crate::memory::{
    default_memory_backend_key, memory_backend_profile, selectable_memory_backends,
};
use crate::providers::{
    canonical_china_provider_name, is_glm_alias, is_glm_cn_alias, is_minimax_alias,
    is_moonshot_alias, is_qianfan_alias, is_qwen_alias, is_qwen_oauth_alias, is_zai_alias,
    is_zai_cn_alias,
};
use anyhow::{bail, Context, Result};
use console::style;
use dialoguer::{Confirm, Input, Select};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod models;
pub(crate) mod steps;
pub use models::run_models_refresh;
#[allow(clippy::wildcard_imports)]
use steps::*;

#[cfg(feature = "ai-protocol")]
fn warn_if_ai_protocol_dir_missing_for_protocol_style_provider(provider_key: &str) {
    if crate::providers::parse_protocol_provider_model(provider_key).is_none() {
        return;
    }
    if crate::protocol_registry::resolve_local_protocol_root().is_some() {
        return;
    }
    println!(
        "{}",
        style(
            "Tip: `provider/model` needs AI_PROTOCOL_DIR set to a local ai-protocol directory (not a URL). \
Without manifests, AiClient::new will fail at runtime — see docs/migration-legacy-to-protocol.md."
        )
        .yellow()
    );
}

// ── Project context collected during wizard ──────────────────────

/// User-provided personalization baked into workspace MD files.
#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    pub user_name: String,
    pub timezone: String,
    pub agent_name: String,
    pub communication_style: String,
}

// ── Banner ───────────────────────────────────────────────────────

const BANNER: &str = r"
    ⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡

    ███████╗███████╗██████╗  ██████╗  ██████╗██╗      █████╗ ██╗    ██╗
    ╚══███╔╝██╔════╝██╔══██╗██╔═══██╗██╔════╝██║     ██╔══██╗██║    ██║
      ███╔╝ █████╗  ██████╔╝██║   ██║██║     ██║     ███████║██║ █╗ ██║
     ███╔╝  ██╔══╝  ██╔══██╗██║   ██║██║     ██║     ██╔══██║██║███╗██║
    ███████╗███████╗██║  ██║╚██████╔╝╚██████╗███████╗██║  ██║╚███╔███╔╝
    ╚══════╝╚══════╝╚═╝  ╚═╝ ╚═════╝  ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝

    Zero overhead. Zero compromise. 100% Rust. 100% Agnostic.

    ⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡
";

const LIVE_MODEL_MAX_OPTIONS: usize = 120;
const MODEL_PREVIEW_LIMIT: usize = 20;
const MODEL_CACHE_FILE: &str = "models_cache.json";
const MODEL_CACHE_TTL_SECS: u64 = 12 * 60 * 60;
const CUSTOM_MODEL_SENTINEL: &str = "__custom_model__";

fn has_launchable_channels(channels: &ChannelsConfig) -> bool {
    let ChannelsConfig {
        cli: _,     // `cli` is always available and does not require channel server startup
        webhook: _, // webhook traffic is handled by gateway, not `velaclaw channel start`
        telegram,
        discord,
        slack,
        mattermost,
        imessage,
        matrix,
        signal,
        whatsapp,
        email,
        irc,
        lark,
        dingtalk,
        linq,
        qq,
        ..
    } = channels;

    telegram.is_some()
        || discord.is_some()
        || slack.is_some()
        || mattermost.is_some()
        || imessage.is_some()
        || matrix.is_some()
        || signal.is_some()
        || whatsapp.is_some()
        || email.is_some()
        || irc.is_some()
        || lark.is_some()
        || dingtalk.is_some()
        || linq.is_some()
        || qq.is_some()
}

// ── Main wizard entry point ──────────────────────────────────────

pub async fn run_wizard(force: bool) -> Result<Config> {
    println!("{}", style(BANNER).cyan().bold());

    println!(
        "  {}",
        style("Welcome to VelaClaw — the fastest, smallest AI assistant.")
            .white()
            .bold()
    );
    println!(
        "  {}",
        style("This wizard will configure your agent in under 60 seconds.").dim()
    );
    println!();

    print_step(1, 9, "Workspace Setup");
    let (workspace_dir, config_path) = setup_workspace()?;
    ensure_onboard_overwrite_allowed(&config_path, force)?;

    print_step(2, 9, "AI Provider & API Key");
    let (provider, api_key, model, provider_api_url) = setup_provider(&workspace_dir)?;

    print_step(3, 9, "Channels (How You Talk to VelaClaw)");
    let channels_config = setup_channels()?;

    print_step(4, 9, "Tunnel (Expose to Internet)");
    let tunnel_config = setup_tunnel()?;

    print_step(5, 9, "Tool Mode & Security");
    let (composio_config, secrets_config) = setup_tool_mode()?;
    let security_profile = setup_security_profile()?;

    print_step(6, 9, "Hardware (Physical World)");
    let hardware_config = setup_hardware()?;

    print_step(7, 9, "Memory Configuration");
    let memory_config = setup_memory()?;

    print_step(8, 9, "Project Context (Personalize Your Agent)");
    let project_ctx = setup_project_context()?;

    print_step(9, 9, "Workspace Files");
    scaffold_workspace(&workspace_dir, &project_ctx)?;

    // ── Build config ──
    // Defaults: SQLite memory, supervised autonomy, workspace-scoped, native runtime
    let config = Config {
        workspace_dir: workspace_dir.clone(),
        config_path: config_path.clone(),
        api_key: if api_key.is_empty() {
            None
        } else {
            Some(api_key)
        },
        api_url: provider_api_url,
        default_provider: Some(provider),
        default_model: Some(model),
        default_temperature: 0.7,
        observability: ObservabilityConfig::default(),
        autonomy: AutonomyConfig::default(),
        runtime: RuntimeConfig::default(),
        reliability: crate::config::ReliabilityConfig::default(),
        scheduler: crate::config::schema::SchedulerConfig::default(),
        agent: crate::config::schema::AgentConfig::default(),
        skills: crate::config::SkillsConfig::default(),
        routing: crate::config::ExecutionRoutingConfig::default(),
        telemetry: crate::config::schema::TelemetryConfig::default(),
        model_routes: Vec::new(),
        embedding_routes: Vec::new(),
        heartbeat: HeartbeatConfig::default(),
        cron: crate::config::CronConfig::default(),
        channels_config,
        memory: memory_config, // User-selected memory backend
        storage: StorageConfig::default(),
        tunnel: tunnel_config,
        gateway: crate::config::GatewayConfig::default(),
        composio: composio_config,
        secrets: secrets_config,
        browser: BrowserConfig::default(),
        http_request: crate::config::HttpRequestConfig::default(),
        multimodal: crate::config::MultimodalConfig::default(),
        web_search: crate::config::WebSearchConfig::default(),
        proxy: crate::config::ProxyConfig::default(),
        identity: crate::config::IdentityConfig::default(),
        cost: crate::config::CostConfig::default(),
        peripherals: crate::config::PeripheralsConfig::default(),
        deploy: crate::config::DeployConfig::default(),
        agents: std::collections::HashMap::new(),
        hardware: hardware_config,
        query_classification: crate::config::QueryClassificationConfig::default(),
        security: crate::config::SecurityConfig {
            profile: Some(security_profile),
            ..crate::config::SecurityConfig::default()
        },
        cli_render: None,
    };

    println!(
        "  {} Security: {} | workspace-scoped",
        style("✓").green().bold(),
        style("Supervised").green()
    );
    println!(
        "  {} Memory: {} (auto-save: {})",
        style("✓").green().bold(),
        style(&config.memory.backend).green(),
        if config.memory.auto_save { "on" } else { "off" }
    );

    config.save().await?;
    persist_workspace_selection(&config.config_path).await?;

    // ── Final summary ────────────────────────────────────────────
    print_summary(&config);

    // ── Offer to launch channels immediately ─────────────────────
    let has_channels = has_launchable_channels(&config.channels_config);

    if has_channels && config.api_key.is_some() {
        let launch: bool = Confirm::new()
            .with_prompt(format!(
                "  {} Launch channels now? (connected channels → AI → reply)",
                style("🚀").cyan()
            ))
            .default(true)
            .interact()?;

        if launch {
            println!();
            println!(
                "  {} {}",
                style("⚡").cyan(),
                style("Starting channel server...").white().bold()
            );
            println!();
            // Signal to main.rs to call start_channels after wizard returns
            std::env::set_var("VELACLAW_AUTOSTART_CHANNELS", "1");
        }
    }

    Ok(config)
}

/// Interactive repair flow: rerun channel setup only without redoing full onboarding.
pub async fn run_channels_repair_wizard() -> Result<Config> {
    println!("{}", style(BANNER).cyan().bold());
    println!(
        "  {}",
        style("Channels Repair — update channel tokens and allowlists only")
            .white()
            .bold()
    );
    println!();

    let mut config = Config::load_or_init().await?;

    print_step(1, 1, "Channels (How You Talk to VelaClaw)");
    config.channels_config = setup_channels()?;
    config.save().await?;
    persist_workspace_selection(&config.config_path).await?;

    println!();
    println!(
        "  {} Channel config saved: {}",
        style("✓").green().bold(),
        style(config.config_path.display()).green()
    );

    let has_channels = has_launchable_channels(&config.channels_config);

    if has_channels && config.api_key.is_some() {
        let launch: bool = Confirm::new()
            .with_prompt(format!(
                "  {} Launch channels now? (connected channels → AI → reply)",
                style("🚀").cyan()
            ))
            .default(true)
            .interact()?;

        if launch {
            println!();
            println!(
                "  {} {}",
                style("⚡").cyan(),
                style("Starting channel server...").white().bold()
            );
            println!();
            // Signal to main.rs to call start_channels after wizard returns
            std::env::set_var("VELACLAW_AUTOSTART_CHANNELS", "1");
        }
    }

    Ok(config)
}

// ── Quick setup (zero prompts) ───────────────────────────────────

/// Non-interactive setup: generates a sensible default config instantly.
/// Use `velaclaw onboard` or `velaclaw onboard --api-key sk-... --provider openai/gpt-5.2 --memory sqlite|lucid`.
/// Use `velaclaw onboard --interactive` for the full wizard.
fn backend_key_from_choice(choice: usize) -> &'static str {
    selectable_memory_backends()
        .get(choice)
        .map_or(default_memory_backend_key(), |backend| backend.key)
}

fn memory_config_defaults_for_backend(backend: &str) -> MemoryConfig {
    let profile = memory_backend_profile(backend);

    MemoryConfig {
        backend: backend.to_string(),
        auto_save: profile.auto_save_default,
        hygiene_enabled: profile.uses_sqlite_hygiene,
        archive_after_days: if profile.uses_sqlite_hygiene { 7 } else { 0 },
        purge_after_days: if profile.uses_sqlite_hygiene { 30 } else { 0 },
        conversation_retention_days: 30,
        embedding_provider: "none".to_string(),
        embedding_model: "text-embedding-3-small".to_string(),
        embedding_dimensions: 1536,
        vector_weight: 0.7,
        keyword_weight: 0.3,
        min_relevance_score: 0.4,
        embedding_cache_size: if profile.uses_sqlite_hygiene {
            10000
        } else {
            0
        },
        chunk_max_tokens: 512,
        response_cache_enabled: false,
        response_cache_ttl_minutes: 60,
        response_cache_max_entries: 5_000,
        snapshot_enabled: false,
        snapshot_on_hygiene: false,
        auto_hydrate: true,
        sqlite_open_timeout_secs: None,
    }
}

#[allow(clippy::too_many_lines)]
pub async fn run_quick_setup(
    credential_override: Option<&str>,
    provider: Option<&str>,
    model_override: Option<&str>,
    memory_backend: Option<&str>,
    force: bool,
) -> Result<Config> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("Could not find home directory")?;

    run_quick_setup_with_home(
        credential_override,
        provider,
        model_override,
        memory_backend,
        force,
        &home,
    )
    .await
}

#[allow(clippy::too_many_lines)]
async fn run_quick_setup_with_home(
    credential_override: Option<&str>,
    provider: Option<&str>,
    model_override: Option<&str>,
    memory_backend: Option<&str>,
    force: bool,
    home: &Path,
) -> Result<Config> {
    // Pin marker root via VELACLAW_CONFIG_DIR under the shared config-resolution
    // env lock so parallel tests never write the operator's real ~/.velaclaw.
    let _env_guard = crate::config::schema::config_resolution_env_lock().await;

    let original_config_dir = std::env::var_os("VELACLAW_CONFIG_DIR");
    let marker_root = home.join(".velaclaw");
    std::env::set_var("VELACLAW_CONFIG_DIR", &marker_root);

    let result = run_quick_setup_with_home_inner(
        credential_override,
        provider,
        model_override,
        memory_backend,
        force,
        home,
    )
    .await;

    match original_config_dir {
        Some(value) => std::env::set_var("VELACLAW_CONFIG_DIR", value),
        None => std::env::remove_var("VELACLAW_CONFIG_DIR"),
    }

    result
}

async fn run_quick_setup_with_home_inner(
    credential_override: Option<&str>,
    provider: Option<&str>,
    model_override: Option<&str>,
    memory_backend: Option<&str>,
    force: bool,
    home: &Path,
) -> Result<Config> {
    println!("{}", style(BANNER).cyan().bold());
    println!(
        "  {}",
        style("Quick Setup — generating config with sensible defaults...")
            .white()
            .bold()
    );
    println!();

    let velaclaw_dir = home.join(".velaclaw");
    let workspace_dir = velaclaw_dir.join("workspace");
    let config_path = velaclaw_dir.join("config.toml");

    ensure_onboard_overwrite_allowed(&config_path, force)?;
    fs::create_dir_all(&workspace_dir).context("Failed to create workspace directory")?;

    let provider_name = provider.unwrap_or(DEFAULT_PROTOCOL_MODEL_ID).to_string();
    #[cfg(feature = "ai-protocol")]
    warn_if_ai_protocol_dir_missing_for_protocol_style_provider(&provider_name);
    let model = model_override
        .map(str::to_string)
        .unwrap_or_else(|| default_model_for_provider(&provider_name));
    let memory_backend_name = memory_backend
        .unwrap_or(default_memory_backend_key())
        .to_string();

    // Create memory config based on backend choice
    let memory_config = memory_config_defaults_for_backend(&memory_backend_name);

    let config = Config {
        workspace_dir: workspace_dir.clone(),
        config_path: config_path.clone(),
        api_key: credential_override.map(|c| {
            let mut s = String::with_capacity(c.len());
            s.push_str(c);
            s
        }),
        api_url: None,
        default_provider: Some(provider_name.clone()),
        default_model: Some(model.clone()),
        default_temperature: 0.7,
        observability: ObservabilityConfig::default(),
        autonomy: AutonomyConfig::default(),
        runtime: RuntimeConfig::default(),
        reliability: crate::config::ReliabilityConfig::default(),
        scheduler: crate::config::schema::SchedulerConfig::default(),
        agent: crate::config::schema::AgentConfig::default(),
        skills: crate::config::SkillsConfig::default(),
        routing: crate::config::ExecutionRoutingConfig::default(),
        telemetry: crate::config::schema::TelemetryConfig::default(),
        model_routes: Vec::new(),
        embedding_routes: Vec::new(),
        heartbeat: HeartbeatConfig::default(),
        cron: crate::config::CronConfig::default(),
        channels_config: ChannelsConfig::default(),
        memory: memory_config,
        storage: StorageConfig::default(),
        tunnel: crate::config::TunnelConfig::default(),
        gateway: crate::config::GatewayConfig::default(),
        composio: ComposioConfig::default(),
        secrets: SecretsConfig::default(),
        browser: BrowserConfig::default(),
        http_request: crate::config::HttpRequestConfig::default(),
        multimodal: crate::config::MultimodalConfig::default(),
        web_search: crate::config::WebSearchConfig::default(),
        proxy: crate::config::ProxyConfig::default(),
        identity: crate::config::IdentityConfig::default(),
        cost: crate::config::CostConfig::default(),
        peripherals: crate::config::PeripheralsConfig::default(),
        deploy: crate::config::DeployConfig::default(),
        agents: std::collections::HashMap::new(),
        hardware: crate::config::HardwareConfig::default(),
        query_classification: crate::config::QueryClassificationConfig::default(),
        security: crate::config::SecurityConfig {
            profile: Some(crate::config::SecurityProfile::Isolated),
            ..crate::config::SecurityConfig::default()
        },
        cli_render: None,
    };

    config.save().await?;
    persist_workspace_selection(&config.config_path).await?;

    // Scaffold minimal workspace files
    let default_ctx = ProjectContext {
        user_name: std::env::var("USER").unwrap_or_else(|_| "User".into()),
        timezone: "UTC".into(),
        agent_name: "VelaClaw".into(),
        communication_style:
            "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing."
                .into(),
    };
    scaffold_workspace(&workspace_dir, &default_ctx)?;

    println!(
        "  {} Workspace:  {}",
        style("✓").green().bold(),
        style(workspace_dir.display()).green()
    );
    println!(
        "  {} Provider:   {}",
        style("✓").green().bold(),
        style(&provider_name).green()
    );
    println!(
        "  {} Model:      {}",
        style("✓").green().bold(),
        style(&model).green()
    );
    println!(
        "  {} API Key:    {}",
        style("✓").green().bold(),
        if credential_override.is_some() {
            style("set").green()
        } else {
            style("not set (use --api-key or edit config.toml)").yellow()
        }
    );
    println!(
        "  {} Security:   {}",
        style("✓").green().bold(),
        style("Supervised (workspace-scoped)").green()
    );
    println!(
        "  {} Memory:     {} (auto-save: {})",
        style("✓").green().bold(),
        style(&memory_backend_name).green(),
        if memory_backend_name == "none" {
            "off"
        } else {
            "on"
        }
    );
    println!(
        "  {} Secrets:    {}",
        style("✓").green().bold(),
        style("encrypted").green()
    );
    println!(
        "  {} Gateway:    {}",
        style("✓").green().bold(),
        style("pairing required (127.0.0.1:8080)").green()
    );
    println!(
        "  {} Tunnel:     {}",
        style("✓").green().bold(),
        style("none (local only)").dim()
    );
    println!(
        "  {} Composio:   {}",
        style("✓").green().bold(),
        style("disabled (sovereign mode)").dim()
    );
    println!();
    println!(
        "  {} {}",
        style("Config saved:").white().bold(),
        style(config_path.display()).green()
    );
    println!();
    println!("  {}", style("Next steps:").white().bold());
    if credential_override.is_none() {
        println!("    1. Set your API key:  export OPENAI_API_KEY=\"sk-...\"");
        println!("    2. Or edit:           ~/.velaclaw/config.toml");
        println!("    3. Chat:              velaclaw agent -m \"Hello!\"");
        println!("    4. Gateway:           velaclaw gateway");
    } else {
        println!("    1. Chat:     velaclaw agent -m \"Hello!\"");
        println!("    2. Gateway:  velaclaw gateway");
        println!("    3. Status:   velaclaw status");
    }
    println!();

    Ok(config)
}

fn canonical_provider_name(provider_name: &str) -> &str {
    if let Some((provider_id, _)) = crate::providers::parse_protocol_provider_model(provider_name) {
        return provider_id;
    }

    if is_qwen_oauth_alias(provider_name) {
        return "qwen-code";
    }

    if let Some(canonical) = canonical_china_provider_name(provider_name) {
        return canonical;
    }

    match provider_name {
        "grok" => "xai",
        "together" => "together-ai",
        "google" | "google-gemini" => "gemini",
        "kimi_coding" | "kimi_for_coding" => "kimi-code",
        "nvidia-nim" | "build.nvidia.com" => "nvidia",
        "aws-bedrock" => "bedrock",
        "llama.cpp" => "llamacpp",
        _ => provider_name,
    }
}

fn allows_unauthenticated_model_fetch(provider_name: &str) -> bool {
    matches!(
        canonical_provider_name(provider_name),
        "openrouter" | "ollama" | "llamacpp" | "venice" | "astrai" | "nvidia"
    )
}

/// Pick a sensible default model for the given provider.
const MINIMAX_ONBOARD_MODELS: [(&str, &str); 5] = [
    ("MiniMax-M2.5", "MiniMax M2.5 (latest, recommended)"),
    ("MiniMax-M2.5-highspeed", "MiniMax M2.5 High-Speed (faster)"),
    ("MiniMax-M2.1", "MiniMax M2.1 (stable)"),
    ("MiniMax-M2.1-highspeed", "MiniMax M2.1 High-Speed (faster)"),
    ("MiniMax-M2", "MiniMax M2 (legacy)"),
];

fn default_model_for_provider(provider: &str) -> String {
    if crate::providers::parse_protocol_provider_model(provider).is_some() {
        return provider.to_string();
    }

    let canonical = canonical_provider_name(provider);
    if let Some(from_manifest) = super::model_catalog::default_model_from_manifest(canonical) {
        return from_manifest;
    }

    curated_default_model_for_provider(canonical)
}

fn curated_default_model_for_provider(canonical: &str) -> String {
    match canonical {
        "anthropic" => "claude-sonnet-4-5-20250929".into(),
        "openai" => "gpt-5.2".into(),
        "openai-codex" => "gpt-5-codex".into(),
        "venice" => "zai-org-glm-5".into(),
        "groq" => "llama-3.3-70b-versatile".into(),
        "mistral" => "mistral-large-latest".into(),
        "deepseek" => "deepseek-chat".into(),
        "xai" => "grok-4-1-fast-reasoning".into(),
        "perplexity" => "sonar-pro".into(),
        "fireworks" => "accounts/fireworks/models/llama-v3p3-70b-instruct".into(),
        "together-ai" => "meta-llama/Llama-3.3-70B-Instruct-Turbo".into(),
        "cohere" => "command-a-03-2025".into(),
        "moonshot" => "kimi-k2.5".into(),
        "glm" | "zai" => "glm-5".into(),
        "minimax" => "MiniMax-M2.5".into(),
        "qwen" => "qwen-plus".into(),
        "qwen-code" => "qwen3-coder-plus".into(),
        "ollama" => "llama3.2".into(),
        "llamacpp" => "ggml-org/gpt-oss-20b-GGUF".into(),
        "gemini" => "gemini-2.5-pro".into(),
        "kimi-code" => "kimi-for-coding".into(),
        "bedrock" => "anthropic.claude-sonnet-4-5-20250929-v1:0".into(),
        "nvidia" => "nvidia/meta/llama-3.3-70b-instruct".into(),
        _ => DEFAULT_PROTOCOL_MODEL_ID.into(),
    }
}

fn curated_models_for_provider(provider_name: &str) -> Vec<(String, String)> {
    let canonical = canonical_provider_name(provider_name);
    if let Some(from_manifest) = super::model_catalog::models_from_manifest(canonical) {
        return from_manifest;
    }

    curated_models_fallback(canonical)
}

fn curated_models_fallback(canonical: &str) -> Vec<(String, String)> {
    match canonical {
        "openrouter" => vec![
            (
                "anthropic/claude-sonnet-4.6".to_string(),
                "Claude Sonnet 4.6 (balanced, recommended)".to_string(),
            ),
            (
                DEFAULT_PROTOCOL_MODEL_ID.to_string(),
                DEFAULT_PROTOCOL_MODEL_LABEL.to_string(),
            ),
            (
                "openai/gpt-5-mini".to_string(),
                "GPT-5 mini (fast, cost-efficient)".to_string(),
            ),
            (
                "google/gemini-3-pro-preview".to_string(),
                "Gemini 3 Pro Preview (frontier reasoning)".to_string(),
            ),
            (
                "x-ai/grok-4.1-fast".to_string(),
                "Grok 4.1 Fast (reasoning + speed)".to_string(),
            ),
            (
                "deepseek/deepseek-v3.2".to_string(),
                "DeepSeek V3.2 (agentic + affordable)".to_string(),
            ),
            (
                "meta-llama/llama-4-maverick".to_string(),
                "Llama 4 Maverick (open model)".to_string(),
            ),
        ],
        "anthropic" => vec![
            (
                "claude-sonnet-4-5-20250929".to_string(),
                "Claude Sonnet 4.5 (balanced, recommended)".to_string(),
            ),
            (
                "claude-opus-4-6".to_string(),
                "Claude Opus 4.6 (best quality)".to_string(),
            ),
            (
                "claude-haiku-4-5-20251001".to_string(),
                "Claude Haiku 4.5 (fastest, cheapest)".to_string(),
            ),
        ],
        "openai" => vec![
            (
                "gpt-5.2".to_string(),
                "GPT-5.2 (latest coding/agentic flagship)".to_string(),
            ),
            (
                "gpt-5-mini".to_string(),
                "GPT-5 mini (faster, cheaper)".to_string(),
            ),
            (
                "gpt-5-nano".to_string(),
                "GPT-5 nano (lowest latency/cost)".to_string(),
            ),
            (
                "gpt-5.2-codex".to_string(),
                "GPT-5.2 Codex (agentic coding)".to_string(),
            ),
        ],
        "openai-codex" => vec![
            (
                "gpt-5-codex".to_string(),
                "GPT-5 Codex (recommended)".to_string(),
            ),
            (
                "gpt-5.2-codex".to_string(),
                "GPT-5.2 Codex (agentic coding)".to_string(),
            ),
            ("o4-mini".to_string(), "o4-mini (fallback)".to_string()),
        ],
        "venice" => vec![
            (
                "zai-org-glm-5".to_string(),
                "GLM-5 via Venice (agentic flagship)".to_string(),
            ),
            (
                "claude-sonnet-4-6".to_string(),
                "Claude Sonnet 4.6 via Venice (best quality)".to_string(),
            ),
            (
                "deepseek-v3.2".to_string(),
                "DeepSeek V3.2 via Venice (strong value)".to_string(),
            ),
            (
                "grok-41-fast".to_string(),
                "Grok 4.1 Fast via Venice (low latency)".to_string(),
            ),
        ],
        "groq" => vec![
            (
                "llama-3.3-70b-versatile".to_string(),
                "Llama 3.3 70B (fast, recommended)".to_string(),
            ),
            (
                "openai/gpt-oss-120b".to_string(),
                "GPT-OSS 120B (strong open-weight)".to_string(),
            ),
            (
                "openai/gpt-oss-20b".to_string(),
                "GPT-OSS 20B (cost-efficient open-weight)".to_string(),
            ),
        ],
        "mistral" => vec![
            (
                "mistral-large-latest".to_string(),
                "Mistral Large (latest flagship)".to_string(),
            ),
            (
                "mistral-medium-latest".to_string(),
                "Mistral Medium (balanced)".to_string(),
            ),
            (
                "codestral-latest".to_string(),
                "Codestral (code-focused)".to_string(),
            ),
            (
                "devstral-latest".to_string(),
                "Devstral (software engineering specialist)".to_string(),
            ),
        ],
        "deepseek" => vec![
            (
                "deepseek-chat".to_string(),
                "DeepSeek Chat (mapped to V3.2 non-thinking)".to_string(),
            ),
            (
                "deepseek-reasoner".to_string(),
                "DeepSeek Reasoner (mapped to V3.2 thinking)".to_string(),
            ),
        ],
        "xai" => vec![
            (
                "grok-4-1-fast-reasoning".to_string(),
                "Grok 4.1 Fast Reasoning (recommended)".to_string(),
            ),
            (
                "grok-4-1-fast-non-reasoning".to_string(),
                "Grok 4.1 Fast Non-Reasoning (low latency)".to_string(),
            ),
            (
                "grok-code-fast-1".to_string(),
                "Grok Code Fast 1 (coding specialist)".to_string(),
            ),
            ("grok-4".to_string(), "Grok 4 (max quality)".to_string()),
        ],
        "perplexity" => vec![
            (
                "sonar-pro".to_string(),
                "Sonar Pro (flagship web-grounded model)".to_string(),
            ),
            (
                "sonar-reasoning-pro".to_string(),
                "Sonar Reasoning Pro (complex multi-step reasoning)".to_string(),
            ),
            (
                "sonar-deep-research".to_string(),
                "Sonar Deep Research (long-form research)".to_string(),
            ),
            ("sonar".to_string(), "Sonar (search, fast)".to_string()),
        ],
        "fireworks" => vec![
            (
                "accounts/fireworks/models/llama-v3p3-70b-instruct".to_string(),
                "Llama 3.3 70B".to_string(),
            ),
            (
                "accounts/fireworks/models/mixtral-8x22b-instruct".to_string(),
                "Mixtral 8x22B".to_string(),
            ),
        ],
        "together-ai" => vec![
            (
                "meta-llama/Llama-3.3-70B-Instruct-Turbo".to_string(),
                "Llama 3.3 70B Instruct Turbo (recommended)".to_string(),
            ),
            (
                "moonshotai/Kimi-K2.5".to_string(),
                "Kimi K2.5 (reasoning + coding)".to_string(),
            ),
            (
                "deepseek-ai/DeepSeek-V3.1".to_string(),
                "DeepSeek V3.1 (strong value)".to_string(),
            ),
        ],
        "cohere" => vec![
            (
                "command-a-03-2025".to_string(),
                "Command A (flagship enterprise model)".to_string(),
            ),
            (
                "command-a-reasoning-08-2025".to_string(),
                "Command A Reasoning (agentic reasoning)".to_string(),
            ),
            (
                "command-r-08-2024".to_string(),
                "Command R (stable fast baseline)".to_string(),
            ),
        ],
        "kimi-code" => vec![
            (
                "kimi-for-coding".to_string(),
                "Kimi for Coding (official coding-agent model)".to_string(),
            ),
            (
                "kimi-k2.5".to_string(),
                "Kimi K2.5 (general coding endpoint model)".to_string(),
            ),
        ],
        "moonshot" => vec![
            (
                "kimi-k2.5".to_string(),
                "Kimi K2.5 (latest flagship, recommended)".to_string(),
            ),
            (
                "kimi-k2-thinking".to_string(),
                "Kimi K2 Thinking (deep reasoning + tool use)".to_string(),
            ),
            (
                "kimi-k2-0905-preview".to_string(),
                "Kimi K2 0905 Preview (strong coding)".to_string(),
            ),
        ],
        "glm" | "zai" => vec![
            ("glm-5".to_string(), "GLM-5 (high reasoning)".to_string()),
            (
                "glm-4.7".to_string(),
                "GLM-4.7 (strong general-purpose quality)".to_string(),
            ),
            (
                "glm-4.5-air".to_string(),
                "GLM-4.5 Air (lower latency)".to_string(),
            ),
        ],
        "minimax" => vec![
            (
                "MiniMax-M2.5".to_string(),
                "MiniMax M2.5 (latest flagship)".to_string(),
            ),
            (
                "MiniMax-M2.5-highspeed".to_string(),
                "MiniMax M2.5 High-Speed (fast)".to_string(),
            ),
            (
                "MiniMax-M2.1".to_string(),
                "MiniMax M2.1 (strong coding/reasoning)".to_string(),
            ),
        ],
        "qwen" => vec![
            (
                "qwen-max".to_string(),
                "Qwen Max (highest quality)".to_string(),
            ),
            (
                "qwen-plus".to_string(),
                "Qwen Plus (balanced default)".to_string(),
            ),
            (
                "qwen-turbo".to_string(),
                "Qwen Turbo (fast and cost-efficient)".to_string(),
            ),
        ],
        "qwen-code" => vec![
            (
                "qwen3-coder-plus".to_string(),
                "Qwen3 Coder Plus (recommended for coding workflows)".to_string(),
            ),
            (
                "qwen3.5-plus".to_string(),
                "Qwen3.5 Plus (reasoning + coding)".to_string(),
            ),
            (
                "qwen3-max-2026-01-23".to_string(),
                "Qwen3 Max (high-capability coding model)".to_string(),
            ),
        ],
        "nvidia" => vec![
            (
                DEFAULT_PROTOCOL_MODEL_ID.to_string(),
                format!("{DEFAULT_PROTOCOL_MODEL_LABEL} (recommended)"),
            ),
            (
                "nvidia/meta/llama-3.3-70b-instruct".to_string(),
                "Llama 3.3 70B Instruct (balanced default)".to_string(),
            ),
            (
                "nvidia/deepseek-ai/deepseek-v3.2".to_string(),
                "DeepSeek V3.2 (advanced reasoning + coding)".to_string(),
            ),
            (
                "nvidia/llama-3.3-nemotron-super-49b-v1.5".to_string(),
                "Llama 3.3 Nemotron Super 49B v1.5 (NVIDIA-tuned)".to_string(),
            ),
            (
                "nvidia/llama-3.1-nemotron-ultra-253b-v1".to_string(),
                "Llama 3.1 Nemotron Ultra 253B v1 (max quality)".to_string(),
            ),
        ],
        "astrai" => vec![
            (
                "anthropic/claude-sonnet-4.6".to_string(),
                "Claude Sonnet 4.6 (balanced default)".to_string(),
            ),
            (
                DEFAULT_PROTOCOL_MODEL_ID.to_string(),
                DEFAULT_PROTOCOL_MODEL_LABEL.to_string(),
            ),
            (
                "deepseek/deepseek-v3.2".to_string(),
                "DeepSeek V3.2 (agentic + affordable)".to_string(),
            ),
            (
                "z-ai/glm-5".to_string(),
                "GLM-5 (high reasoning)".to_string(),
            ),
        ],
        "ollama" => vec![
            (
                "llama3.2".to_string(),
                "Llama 3.2 (recommended local)".to_string(),
            ),
            ("mistral".to_string(), "Mistral 7B".to_string()),
            ("codellama".to_string(), "Code Llama".to_string()),
            ("phi3".to_string(), "Phi-3 (small, fast)".to_string()),
        ],
        "llamacpp" => vec![
            (
                "ggml-org/gpt-oss-20b-GGUF".to_string(),
                "GPT-OSS 20B GGUF (llama.cpp server example)".to_string(),
            ),
            (
                "bartowski/Llama-3.3-70B-Instruct-GGUF".to_string(),
                "Llama 3.3 70B GGUF (high quality)".to_string(),
            ),
            (
                "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF".to_string(),
                "Qwen2.5 Coder 7B GGUF (coding-focused)".to_string(),
            ),
        ],
        "bedrock" => vec![
            (
                "anthropic.claude-sonnet-4-6".to_string(),
                "Claude Sonnet 4.6 (latest, recommended)".to_string(),
            ),
            (
                "anthropic.claude-opus-4-6-v1".to_string(),
                "Claude Opus 4.6 (strongest)".to_string(),
            ),
            (
                "anthropic.claude-haiku-4-5-20251001-v1:0".to_string(),
                "Claude Haiku 4.5 (fastest, cheapest)".to_string(),
            ),
            (
                "anthropic.claude-sonnet-4-5-20250929-v1:0".to_string(),
                "Claude Sonnet 4.5".to_string(),
            ),
        ],
        "gemini" => vec![
            (
                "gemini-3-pro-preview".to_string(),
                "Gemini 3 Pro Preview (latest frontier reasoning)".to_string(),
            ),
            (
                "gemini-2.5-pro".to_string(),
                "Gemini 2.5 Pro (stable reasoning)".to_string(),
            ),
            (
                "gemini-2.5-flash".to_string(),
                "Gemini 2.5 Flash (best price/performance)".to_string(),
            ),
            (
                "gemini-2.5-flash-lite".to_string(),
                "Gemini 2.5 Flash-Lite (lowest cost)".to_string(),
            ),
        ],
        _ => vec![("default".to_string(), "Default model".to_string())],
    }
}

fn supports_live_model_fetch(provider_name: &str) -> bool {
    matches!(
        canonical_provider_name(provider_name),
        "openrouter"
            | "openai"
            | "anthropic"
            | "groq"
            | "mistral"
            | "deepseek"
            | "xai"
            | "together-ai"
            | "gemini"
            | "ollama"
            | "llamacpp"
            | "astrai"
            | "venice"
            | "fireworks"
            | "cohere"
            | "moonshot"
            | "glm"
            | "zai"
            | "qwen"
            | "nvidia"
    )
}

fn models_endpoint_for_provider(provider_name: &str) -> Option<&'static str> {
    match provider_name {
        "qwen-intl" => Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1/models"),
        "dashscope-us" => Some("https://dashscope-us.aliyuncs.com/compatible-mode/v1/models"),
        "moonshot-cn" | "kimi-cn" => Some("https://api.moonshot.cn/v1/models"),
        "glm-cn" | "bigmodel" => Some("https://open.bigmodel.cn/api/paas/v4/models"),
        "zai-cn" | "z.ai-cn" => Some("https://open.bigmodel.cn/api/coding/paas/v4/models"),
        _ => match canonical_provider_name(provider_name) {
            "openai" => Some("https://api.openai.com/v1/models"),
            "venice" => Some("https://api.venice.ai/api/v1/models"),
            "groq" => Some("https://api.groq.com/openai/v1/models"),
            "mistral" => Some("https://api.mistral.ai/v1/models"),
            "deepseek" => Some("https://api.deepseek.com/v1/models"),
            "xai" => Some("https://api.x.ai/v1/models"),
            "together-ai" => Some("https://api.together.xyz/v1/models"),
            "fireworks" => Some("https://api.fireworks.ai/inference/v1/models"),
            "cohere" => Some("https://api.cohere.com/compatibility/v1/models"),
            "moonshot" => Some("https://api.moonshot.ai/v1/models"),
            "glm" => Some("https://api.z.ai/api/paas/v4/models"),
            "zai" => Some("https://api.z.ai/api/coding/paas/v4/models"),
            "qwen" => Some("https://dashscope.aliyuncs.com/compatible-mode/v1/models"),
            "nvidia" => Some("https://integrate.api.nvidia.com/v1/models"),
            "astrai" => Some("https://as-trai.com/v1/models"),
            "llamacpp" => Some("http://localhost:8080/v1/models"),
            _ => None,
        },
    }
}

fn build_model_fetch_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(4))
        .build()
        .context("failed to build model-fetch HTTP client")
}

fn normalize_model_ids(ids: Vec<String>) -> Vec<String> {
    let mut unique = BTreeSet::new();
    for id in ids {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            unique.insert(trimmed.to_string());
        }
    }
    unique.into_iter().collect()
}

fn parse_openai_compatible_model_ids(payload: &Value) -> Vec<String> {
    let mut models = Vec::new();

    if let Some(data) = payload.get("data").and_then(Value::as_array) {
        for model in data {
            if let Some(id) = model.get("id").and_then(Value::as_str) {
                models.push(id.to_string());
            }
        }
    } else if let Some(data) = payload.as_array() {
        for model in data {
            if let Some(id) = model.get("id").and_then(Value::as_str) {
                models.push(id.to_string());
            }
        }
    }

    normalize_model_ids(models)
}

fn parse_gemini_model_ids(payload: &Value) -> Vec<String> {
    let Some(models) = payload.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut ids = Vec::new();
    for model in models {
        let supports_generate_content = model
            .get("supportedGenerationMethods")
            .and_then(Value::as_array)
            .is_none_or(|methods| {
                methods
                    .iter()
                    .any(|method| method.as_str() == Some("generateContent"))
            });

        if !supports_generate_content {
            continue;
        }

        if let Some(name) = model.get("name").and_then(Value::as_str) {
            ids.push(name.trim_start_matches("models/").to_string());
        }
    }

    normalize_model_ids(ids)
}

fn parse_ollama_model_ids(payload: &Value) -> Vec<String> {
    let Some(models) = payload.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut ids = Vec::new();
    for model in models {
        if let Some(name) = model.get("name").and_then(Value::as_str) {
            ids.push(name.to_string());
        }
    }

    normalize_model_ids(ids)
}

fn fetch_openai_compatible_models(
    endpoint: &str,
    api_key: Option<&str>,
    allow_unauthenticated: bool,
) -> Result<Vec<String>> {
    let client = build_model_fetch_client()?;
    let mut request = client.get(endpoint);

    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    } else if !allow_unauthenticated {
        bail!("model fetch requires API key for endpoint {endpoint}");
    }

    let payload: Value = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .with_context(|| format!("model fetch failed: GET {endpoint}"))?
        .json()
        .context("failed to parse model list response")?;

    Ok(parse_openai_compatible_model_ids(&payload))
}

fn fetch_openrouter_models(api_key: Option<&str>) -> Result<Vec<String>> {
    let client = build_model_fetch_client()?;
    let mut request = client.get("https://openrouter.ai/api/v1/models");
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }

    let payload: Value = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .context("model fetch failed: GET https://openrouter.ai/api/v1/models")?
        .json()
        .context("failed to parse OpenRouter model list response")?;

    Ok(parse_openai_compatible_model_ids(&payload))
}

fn fetch_anthropic_models(api_key: Option<&str>) -> Result<Vec<String>> {
    let Some(api_key) = api_key else {
        bail!("Anthropic model fetch requires API key or OAuth token");
    };

    let client = build_model_fetch_client()?;
    let mut request = client
        .get("https://api.anthropic.com/v1/models")
        .header("anthropic-version", "2023-06-01");

    if api_key.starts_with("sk-ant-oat01-") {
        request = request
            .header("Authorization", format!("Bearer {api_key}"))
            .header("anthropic-beta", "oauth-2025-04-20");
    } else {
        request = request.header("x-api-key", api_key);
    }

    let response = request
        .send()
        .context("model fetch failed: GET https://api.anthropic.com/v1/models")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        bail!("Anthropic model list request failed (HTTP {status}): {body}");
    }

    let payload: Value = response
        .json()
        .context("failed to parse Anthropic model list response")?;

    Ok(parse_openai_compatible_model_ids(&payload))
}

fn fetch_gemini_models(api_key: Option<&str>) -> Result<Vec<String>> {
    let Some(api_key) = api_key else {
        bail!("Gemini model fetch requires API key");
    };

    let client = build_model_fetch_client()?;
    let payload: Value = client
        .get("https://generativelanguage.googleapis.com/v1beta/models")
        .query(&[("key", api_key), ("pageSize", "200")])
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .context("model fetch failed: GET Gemini models")?
        .json()
        .context("failed to parse Gemini model list response")?;

    Ok(parse_gemini_model_ids(&payload))
}

fn fetch_ollama_models() -> Result<Vec<String>> {
    let client = build_model_fetch_client()?;
    let payload: Value = client
        .get("http://localhost:11434/api/tags")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .context("model fetch failed: GET http://localhost:11434/api/tags")?
        .json()
        .context("failed to parse Ollama model list response")?;

    Ok(parse_ollama_model_ids(&payload))
}

fn normalize_ollama_endpoint_url(raw_url: &str) -> String {
    let trimmed = raw_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .strip_suffix("/api")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string()
}

fn ollama_endpoint_is_local(endpoint_url: &str) -> bool {
    reqwest::Url::parse(endpoint_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "0.0.0.0"))
}

fn ollama_uses_remote_endpoint(provider_api_url: Option<&str>) -> bool {
    let Some(endpoint) = provider_api_url else {
        return false;
    };

    let normalized = normalize_ollama_endpoint_url(endpoint);
    if normalized.is_empty() {
        return false;
    }

    !ollama_endpoint_is_local(&normalized)
}

fn resolve_live_models_endpoint(
    provider_name: &str,
    provider_api_url: Option<&str>,
) -> Option<String> {
    if canonical_provider_name(provider_name) == "llamacpp" {
        if let Some(url) = provider_api_url
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            let normalized = url.trim_end_matches('/');
            if normalized.ends_with("/models") {
                return Some(normalized.to_string());
            }
            return Some(format!("{normalized}/models"));
        }
    }

    models_endpoint_for_provider(provider_name).map(str::to_string)
}

fn fetch_live_models_for_provider(
    provider_name: &str,
    api_key: &str,
    provider_api_url: Option<&str>,
) -> Result<Vec<String>> {
    let requested_provider_name = provider_name;
    let provider_name = canonical_provider_name(provider_name);
    let ollama_remote = provider_name == "ollama" && ollama_uses_remote_endpoint(provider_api_url);
    let api_key = if api_key.trim().is_empty() {
        if provider_name == "ollama" && !ollama_remote {
            None
        } else {
            std::env::var(provider_env_var(provider_name))
                .ok()
                .or_else(|| {
                    // Anthropic also accepts OAuth setup-tokens via ANTHROPIC_OAUTH_TOKEN
                    if provider_name == "anthropic" {
                        std::env::var("ANTHROPIC_OAUTH_TOKEN").ok()
                    } else if provider_name == "minimax" {
                        std::env::var("MINIMAX_OAUTH_TOKEN").ok()
                    } else {
                        None
                    }
                })
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        }
    } else {
        Some(api_key.trim().to_string())
    };

    let models = match provider_name {
        "openrouter" => fetch_openrouter_models(api_key.as_deref())?,
        "anthropic" => fetch_anthropic_models(api_key.as_deref())?,
        "gemini" => fetch_gemini_models(api_key.as_deref())?,
        "ollama" => {
            if ollama_remote {
                // Remote Ollama endpoints can serve cloud-routed models.
                // Keep this curated list aligned with current Ollama cloud catalog.
                vec![
                    "glm-5:cloud".to_string(),
                    "glm-4.7:cloud".to_string(),
                    "gpt-oss:20b:cloud".to_string(),
                    "gpt-oss:120b:cloud".to_string(),
                    "gemini-3-flash-preview:cloud".to_string(),
                    "qwen3-coder-next:cloud".to_string(),
                    "qwen3-coder:480b:cloud".to_string(),
                    "kimi-k2.5:cloud".to_string(),
                    "minimax-m2.5:cloud".to_string(),
                    "deepseek-v3.1:671b:cloud".to_string(),
                ]
            } else {
                // Local endpoints should not surface cloud-only suffixes.
                fetch_ollama_models()?
                    .into_iter()
                    .filter(|model_id| !model_id.ends_with(":cloud"))
                    .collect()
            }
        }
        _ => {
            if let Some(endpoint) =
                resolve_live_models_endpoint(requested_provider_name, provider_api_url)
            {
                let allow_unauthenticated =
                    allows_unauthenticated_model_fetch(requested_provider_name);
                fetch_openai_compatible_models(
                    &endpoint,
                    api_key.as_deref(),
                    allow_unauthenticated,
                )?
            } else {
                Vec::new()
            }
        }
    };

    Ok(models)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelCacheEntry {
    provider: String,
    fetched_at_unix: u64,
    models: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ModelCacheState {
    entries: Vec<ModelCacheEntry>,
}

#[derive(Debug, Clone)]
struct CachedModels {
    models: Vec<String>,
    age_secs: u64,
}

fn model_cache_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("state").join(MODEL_CACHE_FILE)
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn load_model_cache_state(workspace_dir: &Path) -> Result<ModelCacheState> {
    let path = model_cache_path(workspace_dir);
    if !path.exists() {
        return Ok(ModelCacheState::default());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read model cache at {}", path.display()))?;

    match serde_json::from_str::<ModelCacheState>(&raw) {
        Ok(state) => Ok(state),
        Err(_) => Ok(ModelCacheState::default()),
    }
}

fn save_model_cache_state(workspace_dir: &Path, state: &ModelCacheState) -> Result<()> {
    let path = model_cache_path(workspace_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create model cache directory {}",
                parent.display()
            )
        })?;
    }

    let json = serde_json::to_vec_pretty(state).context("failed to serialize model cache")?;
    fs::write(&path, json)
        .with_context(|| format!("failed to write model cache at {}", path.display()))?;

    Ok(())
}

fn cache_live_models_for_provider(
    workspace_dir: &Path,
    provider_name: &str,
    models: &[String],
) -> Result<()> {
    let normalized_models = normalize_model_ids(models.to_vec());
    if normalized_models.is_empty() {
        return Ok(());
    }

    let mut state = load_model_cache_state(workspace_dir)?;
    let now = now_unix_secs();

    if let Some(entry) = state
        .entries
        .iter_mut()
        .find(|entry| entry.provider == provider_name)
    {
        entry.fetched_at_unix = now;
        entry.models = normalized_models;
    } else {
        state.entries.push(ModelCacheEntry {
            provider: provider_name.to_string(),
            fetched_at_unix: now,
            models: normalized_models,
        });
    }

    save_model_cache_state(workspace_dir, &state)
}

fn load_cached_models_for_provider_internal(
    workspace_dir: &Path,
    provider_name: &str,
    ttl_secs: Option<u64>,
) -> Result<Option<CachedModels>> {
    let state = load_model_cache_state(workspace_dir)?;
    let now = now_unix_secs();

    let Some(entry) = state
        .entries
        .into_iter()
        .find(|entry| entry.provider == provider_name)
    else {
        return Ok(None);
    };

    if entry.models.is_empty() {
        return Ok(None);
    }

    let age_secs = now.saturating_sub(entry.fetched_at_unix);
    if ttl_secs.is_some_and(|ttl| age_secs > ttl) {
        return Ok(None);
    }

    Ok(Some(CachedModels {
        models: entry.models,
        age_secs,
    }))
}

fn load_cached_models_for_provider(
    workspace_dir: &Path,
    provider_name: &str,
    ttl_secs: u64,
) -> Result<Option<CachedModels>> {
    load_cached_models_for_provider_internal(workspace_dir, provider_name, Some(ttl_secs))
}

fn load_any_cached_models_for_provider(
    workspace_dir: &Path,
    provider_name: &str,
) -> Result<Option<CachedModels>> {
    load_cached_models_for_provider_internal(workspace_dir, provider_name, None)
}

fn humanize_age(age_secs: u64) -> String {
    if age_secs < 60 {
        format!("{age_secs}s")
    } else if age_secs < 60 * 60 {
        format!("{}m", age_secs / 60)
    } else {
        format!("{}h", age_secs / (60 * 60))
    }
}

fn build_model_options(model_ids: Vec<String>, source: &str) -> Vec<(String, String)> {
    model_ids
        .into_iter()
        .map(|model_id| {
            let label = format!("{model_id} ({source})");
            (model_id, label)
        })
        .collect()
}

fn print_model_preview(models: &[String]) {
    for model in models.iter().take(MODEL_PREVIEW_LIMIT) {
        println!("  {} {model}", style("-"));
    }

    if models.len() > MODEL_PREVIEW_LIMIT {
        println!(
            "  {} ... and {} more",
            style("-"),
            models.len() - MODEL_PREVIEW_LIMIT
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_PROTOCOL_MODEL_ID;
    use serde_json::json;
    use tempfile::TempDir;

    // ── ProjectContext defaults ──────────────────────────────────

    #[test]
    fn project_context_default_is_empty() {
        let ctx = ProjectContext::default();
        assert!(ctx.user_name.is_empty());
        assert!(ctx.timezone.is_empty());
        assert!(ctx.agent_name.is_empty());
        assert!(ctx.communication_style.is_empty());
    }

    #[tokio::test]
    async fn quick_setup_model_override_persists_to_config_toml() {
        let tmp = TempDir::new().unwrap();

        let config = run_quick_setup_with_home(
            Some("sk-issue946"),
            Some(DEFAULT_PROTOCOL_MODEL_ID),
            Some("custom-model-946"),
            Some("sqlite"),
            false,
            tmp.path(),
        )
        .await
        .unwrap();

        assert_eq!(
            config.default_provider.as_deref(),
            Some(DEFAULT_PROTOCOL_MODEL_ID)
        );
        assert_eq!(config.default_model.as_deref(), Some("custom-model-946"));
        assert_eq!(config.api_key.as_deref(), Some("sk-issue946"));

        let config_raw = tokio::fs::read_to_string(config.config_path).await.unwrap();
        let expected_line = format!("default_provider = \"{DEFAULT_PROTOCOL_MODEL_ID}\"");
        assert!(config_raw.contains(&expected_line));
        assert!(config_raw.contains("default_model = \"custom-model-946\""));
    }

    #[tokio::test]
    async fn quick_setup_without_model_uses_provider_default_model() {
        let tmp = TempDir::new().unwrap();

        let config = run_quick_setup_with_home(
            Some("sk-issue946"),
            Some("anthropic/claude-sonnet-4-5-20250929"),
            None,
            Some("sqlite"),
            false,
            tmp.path(),
        )
        .await
        .unwrap();

        let expected = default_model_for_provider("anthropic/claude-sonnet-4-5-20250929");
        assert_eq!(
            config.default_provider.as_deref(),
            Some("anthropic/claude-sonnet-4-5-20250929")
        );
        assert_eq!(config.default_model.as_deref(), Some(expected.as_str()));
    }

    #[tokio::test]
    async fn quick_setup_existing_config_requires_force_when_non_interactive() {
        let tmp = TempDir::new().unwrap();
        let velaclaw_dir = tmp.path().join(".velaclaw");
        let config_path = velaclaw_dir.join("config.toml");

        tokio::fs::create_dir_all(&velaclaw_dir).await.unwrap();
        tokio::fs::write(
            &config_path,
            format!("default_provider = \"{DEFAULT_PROTOCOL_MODEL_ID}\"\n"),
        )
        .await
        .unwrap();

        let err = run_quick_setup_with_home(
            Some("sk-existing"),
            Some(DEFAULT_PROTOCOL_MODEL_ID),
            Some("custom-model"),
            Some("sqlite"),
            false,
            tmp.path(),
        )
        .await
        .expect_err("quick setup should refuse overwrite without --force");

        let err_text = err.to_string();
        assert!(err_text.contains("Refusing to overwrite existing config"));
        assert!(err_text.contains("--force"));
    }

    #[tokio::test]
    async fn quick_setup_existing_config_overwrites_with_force() {
        let tmp = TempDir::new().unwrap();
        let velaclaw_dir = tmp.path().join(".velaclaw");
        let config_path = velaclaw_dir.join("config.toml");

        tokio::fs::create_dir_all(&velaclaw_dir).await.unwrap();
        tokio::fs::write(
            &config_path,
            "default_provider = \"anthropic\"\ndefault_model = \"stale-model\"\n",
        )
        .await
        .unwrap();

        let config = run_quick_setup_with_home(
            Some("sk-force"),
            Some(DEFAULT_PROTOCOL_MODEL_ID),
            Some("custom-model-fresh"),
            Some("sqlite"),
            true,
            tmp.path(),
        )
        .await
        .expect("quick setup should overwrite existing config with --force");

        assert_eq!(
            config.default_provider.as_deref(),
            Some(DEFAULT_PROTOCOL_MODEL_ID)
        );
        assert_eq!(config.default_model.as_deref(), Some("custom-model-fresh"));
        assert_eq!(config.api_key.as_deref(), Some("sk-force"));

        let config_raw = tokio::fs::read_to_string(config.config_path).await.unwrap();
        let expected_line = format!("default_provider = \"{DEFAULT_PROTOCOL_MODEL_ID}\"");
        assert!(config_raw.contains(&expected_line));
        assert!(config_raw.contains("default_model = \"custom-model-fresh\""));
    }

    // ── scaffold_workspace: basic file creation ─────────────────

    #[test]
    fn scaffold_creates_all_md_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let expected = [
            "IDENTITY.md",
            "AGENTS.md",
            "HEARTBEAT.md",
            "SOUL.md",
            "USER.md",
            "TOOLS.md",
            "BOOTSTRAP.md",
            "MEMORY.md",
            "agent-policy.yaml",
        ];
        for f in &expected {
            assert!(tmp.path().join(f).exists(), "missing file: {f}");
        }
        let policy = std::fs::read_to_string(tmp.path().join("agent-policy.yaml")).unwrap();
        assert!(policy.contains("autonomy.allowed_commands"));
        assert!(policy.contains("self_adjust"));
    }

    #[test]
    fn scaffold_creates_all_subdirectories() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        for dir in &["sessions", "memory", "state", "cron", "skills"] {
            assert!(tmp.path().join(dir).is_dir(), "missing subdirectory: {dir}");
        }
    }

    // ── scaffold_workspace: personalization ─────────────────────

    #[tokio::test]
    async fn scaffold_bakes_user_name_into_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "Alice".into(),
            ..Default::default()
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let user_md = tokio::fs::read_to_string(tmp.path().join("USER.md"))
            .await
            .unwrap();
        assert!(
            user_md.contains("**Name:** Alice"),
            "USER.md should contain user name"
        );

        let bootstrap = tokio::fs::read_to_string(tmp.path().join("BOOTSTRAP.md"))
            .await
            .unwrap();
        assert!(
            bootstrap.contains("**Alice**"),
            "BOOTSTRAP.md should contain user name"
        );
    }

    #[tokio::test]
    async fn scaffold_bakes_timezone_into_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            timezone: "US/Pacific".into(),
            ..Default::default()
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let user_md = tokio::fs::read_to_string(tmp.path().join("USER.md"))
            .await
            .unwrap();
        assert!(
            user_md.contains("**Timezone:** US/Pacific"),
            "USER.md should contain timezone"
        );

        let bootstrap = tokio::fs::read_to_string(tmp.path().join("BOOTSTRAP.md"))
            .await
            .unwrap();
        assert!(
            bootstrap.contains("US/Pacific"),
            "BOOTSTRAP.md should contain timezone"
        );
    }

    #[tokio::test]
    async fn scaffold_bakes_agent_name_into_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            agent_name: "Crabby".into(),
            ..Default::default()
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let identity = tokio::fs::read_to_string(tmp.path().join("IDENTITY.md"))
            .await
            .unwrap();
        assert!(
            identity.contains("**Name:** Crabby"),
            "IDENTITY.md should contain agent name"
        );

        let soul = tokio::fs::read_to_string(tmp.path().join("SOUL.md"))
            .await
            .unwrap();
        assert!(
            soul.contains("You are **Crabby**"),
            "SOUL.md should contain agent name"
        );

        let agents = tokio::fs::read_to_string(tmp.path().join("AGENTS.md"))
            .await
            .unwrap();
        assert!(
            agents.contains("Crabby Personal Assistant"),
            "AGENTS.md should contain agent name"
        );

        let heartbeat = tokio::fs::read_to_string(tmp.path().join("HEARTBEAT.md"))
            .await
            .unwrap();
        assert!(
            heartbeat.contains("Crabby"),
            "HEARTBEAT.md should contain agent name"
        );

        let bootstrap = tokio::fs::read_to_string(tmp.path().join("BOOTSTRAP.md"))
            .await
            .unwrap();
        assert!(
            bootstrap.contains("Introduce yourself as Crabby"),
            "BOOTSTRAP.md should contain agent name"
        );
    }

    #[tokio::test]
    async fn scaffold_bakes_communication_style() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            communication_style: "Be technical and detailed.".into(),
            ..Default::default()
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let soul = tokio::fs::read_to_string(tmp.path().join("SOUL.md"))
            .await
            .unwrap();
        assert!(
            soul.contains("Be technical and detailed."),
            "SOUL.md should contain communication style"
        );

        let user_md = tokio::fs::read_to_string(tmp.path().join("USER.md"))
            .await
            .unwrap();
        assert!(
            user_md.contains("Be technical and detailed."),
            "USER.md should contain communication style"
        );

        let bootstrap = tokio::fs::read_to_string(tmp.path().join("BOOTSTRAP.md"))
            .await
            .unwrap();
        assert!(
            bootstrap.contains("Be technical and detailed."),
            "BOOTSTRAP.md should contain communication style"
        );
    }

    // ── scaffold_workspace: defaults when context is empty ──────

    #[tokio::test]
    async fn scaffold_uses_defaults_for_empty_context() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default(); // all empty
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let identity = tokio::fs::read_to_string(tmp.path().join("IDENTITY.md"))
            .await
            .unwrap();
        assert!(
            identity.contains("**Name:** VelaClaw"),
            "should default agent name to VelaClaw"
        );

        let user_md = tokio::fs::read_to_string(tmp.path().join("USER.md"))
            .await
            .unwrap();
        assert!(
            user_md.contains("**Name:** User"),
            "should default user name to User"
        );
        assert!(
            user_md.contains("**Timezone:** UTC"),
            "should default timezone to UTC"
        );

        let soul = tokio::fs::read_to_string(tmp.path().join("SOUL.md"))
            .await
            .unwrap();
        assert!(
            soul.contains("Be warm, natural, and clear."),
            "should default communication style"
        );
    }

    // ── scaffold_workspace: skip existing files ─────────────────

    #[tokio::test]
    async fn scaffold_does_not_overwrite_existing_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "Bob".into(),
            ..Default::default()
        };

        // Pre-create SOUL.md with custom content
        let soul_path = tmp.path().join("SOUL.md");
        fs::write(&soul_path, "# My Custom Soul\nDo not overwrite me.").unwrap();

        scaffold_workspace(tmp.path(), &ctx).unwrap();

        // SOUL.md should be untouched
        let soul = tokio::fs::read_to_string(&soul_path).await.unwrap();
        assert!(
            soul.contains("Do not overwrite me"),
            "existing files should not be overwritten"
        );
        assert!(
            !soul.contains("You're not a chatbot"),
            "should not contain scaffold content"
        );

        // But USER.md should be created fresh
        let user_md = tokio::fs::read_to_string(tmp.path().join("USER.md"))
            .await
            .unwrap();
        assert!(user_md.contains("**Name:** Bob"));
    }

    // ── scaffold_workspace: idempotent ──────────────────────────

    #[tokio::test]
    async fn scaffold_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "Eve".into(),
            agent_name: "Claw".into(),
            ..Default::default()
        };

        scaffold_workspace(tmp.path(), &ctx).unwrap();
        let soul_v1 = tokio::fs::read_to_string(tmp.path().join("SOUL.md"))
            .await
            .unwrap();

        // Run again — should not change anything
        scaffold_workspace(tmp.path(), &ctx).unwrap();
        let soul_v2 = tokio::fs::read_to_string(tmp.path().join("SOUL.md"))
            .await
            .unwrap();

        assert_eq!(soul_v1, soul_v2, "scaffold should be idempotent");
    }

    // ── scaffold_workspace: all files are non-empty ─────────────

    #[tokio::test]
    async fn scaffold_files_are_non_empty() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        for f in &[
            "IDENTITY.md",
            "AGENTS.md",
            "HEARTBEAT.md",
            "SOUL.md",
            "USER.md",
            "TOOLS.md",
            "BOOTSTRAP.md",
            "MEMORY.md",
            "agent-policy.yaml",
        ] {
            let content = tokio::fs::read_to_string(tmp.path().join(f)).await.unwrap();
            assert!(!content.trim().is_empty(), "{f} should not be empty");
        }
    }

    // ── scaffold_workspace: AGENTS.md references on-demand memory

    #[tokio::test]
    async fn agents_md_references_on_demand_memory() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let agents = tokio::fs::read_to_string(tmp.path().join("AGENTS.md"))
            .await
            .unwrap();
        assert!(
            agents.contains("memory_recall"),
            "AGENTS.md should reference memory_recall for on-demand access"
        );
        assert!(
            agents.contains("on-demand"),
            "AGENTS.md should mention daily notes are on-demand"
        );
    }

    // ── scaffold_workspace: MEMORY.md warns about token cost ────

    #[tokio::test]
    async fn memory_md_warns_about_token_cost() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let memory = tokio::fs::read_to_string(tmp.path().join("MEMORY.md"))
            .await
            .unwrap();
        assert!(
            memory.contains("costs tokens"),
            "MEMORY.md should warn about token cost"
        );
        assert!(
            memory.contains("auto-injected"),
            "MEMORY.md should mention it's auto-injected"
        );
    }

    // ── scaffold_workspace: TOOLS.md lists memory_forget ────────

    #[tokio::test]
    async fn tools_md_lists_all_builtin_tools() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let tools = tokio::fs::read_to_string(tmp.path().join("TOOLS.md"))
            .await
            .unwrap();
        for tool in &[
            "shell",
            "file_read",
            "file_write",
            "memory_store",
            "memory_recall",
            "memory_forget",
        ] {
            assert!(
                tools.contains(tool),
                "TOOLS.md should list built-in tool: {tool}"
            );
        }
        assert!(
            tools.contains("Use when:"),
            "TOOLS.md should include 'Use when' guidance"
        );
        assert!(
            tools.contains("Don't use when:"),
            "TOOLS.md should include 'Don't use when' guidance"
        );
    }

    #[tokio::test]
    async fn soul_md_includes_emoji_awareness_guidance() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let soul = tokio::fs::read_to_string(tmp.path().join("SOUL.md"))
            .await
            .unwrap();
        assert!(
            soul.contains("Use emojis naturally (0-2 max"),
            "SOUL.md should include emoji usage guidance"
        );
        assert!(
            soul.contains("Match emoji density to the user"),
            "SOUL.md should include emoji-awareness guidance"
        );
    }

    // ── scaffold_workspace: special characters in names ─────────

    #[tokio::test]
    async fn scaffold_handles_special_characters_in_names() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "José María".into(),
            agent_name: "VelaClaw-v2".into(),
            timezone: "Europe/Madrid".into(),
            communication_style: "Be direct.".into(),
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let user_md = tokio::fs::read_to_string(tmp.path().join("USER.md"))
            .await
            .unwrap();
        assert!(user_md.contains("José María"));

        let soul = tokio::fs::read_to_string(tmp.path().join("SOUL.md"))
            .await
            .unwrap();
        assert!(soul.contains("VelaClaw-v2"));
    }

    // ── scaffold_workspace: full personalization round-trip ─────

    #[tokio::test]
    async fn scaffold_full_personalization() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "Argenis".into(),
            timezone: "US/Eastern".into(),
            agent_name: "Claw".into(),
            communication_style:
                "Be friendly, human, and conversational. Show warmth and empathy while staying efficient. Use natural contractions."
                    .into(),
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        // Verify every file got personalized
        let identity = tokio::fs::read_to_string(tmp.path().join("IDENTITY.md"))
            .await
            .unwrap();
        assert!(identity.contains("**Name:** Claw"));

        let soul = tokio::fs::read_to_string(tmp.path().join("SOUL.md"))
            .await
            .unwrap();
        assert!(soul.contains("You are **Claw**"));
        assert!(soul.contains("Be friendly, human, and conversational"));

        let user_md = tokio::fs::read_to_string(tmp.path().join("USER.md"))
            .await
            .unwrap();
        assert!(user_md.contains("**Name:** Argenis"));
        assert!(user_md.contains("**Timezone:** US/Eastern"));
        assert!(user_md.contains("Be friendly, human, and conversational"));

        let agents = tokio::fs::read_to_string(tmp.path().join("AGENTS.md"))
            .await
            .unwrap();
        assert!(agents.contains("Claw Personal Assistant"));

        let bootstrap = tokio::fs::read_to_string(tmp.path().join("BOOTSTRAP.md"))
            .await
            .unwrap();
        assert!(bootstrap.contains("**Argenis**"));
        assert!(bootstrap.contains("US/Eastern"));
        assert!(bootstrap.contains("Introduce yourself as Claw"));

        let heartbeat = tokio::fs::read_to_string(tmp.path().join("HEARTBEAT.md"))
            .await
            .unwrap();
        assert!(heartbeat.contains("Claw"));
    }

    // ── model helper coverage ───────────────────────────────────

    #[test]
    fn default_model_for_provider_uses_latest_defaults() {
        // Curated offline defaults (manifest path is env-dependent via AI_PROTOCOL_DIR).
        let curated =
            |provider: &str| curated_default_model_for_provider(canonical_provider_name(provider));
        assert_eq!(curated("openai"), "gpt-5.2");
        assert_eq!(curated("openai-codex"), "gpt-5-codex");
        assert_eq!(curated("anthropic"), "claude-sonnet-4-5-20250929");
        assert_eq!(curated("qwen"), "qwen-plus");
        assert_eq!(curated("qwen-intl"), "qwen-plus");
        assert_eq!(curated("qwen-code"), "qwen3-coder-plus");
        assert_eq!(curated("glm-cn"), "glm-5");
        assert_eq!(curated("minimax-cn"), "MiniMax-M2.5");
        assert_eq!(curated("zai-cn"), "glm-5");
        assert_eq!(curated("gemini"), "gemini-2.5-pro");
        assert_eq!(curated("google"), "gemini-2.5-pro");
        assert_eq!(curated("kimi-code"), "kimi-for-coding");
        assert_eq!(
            curated("bedrock"),
            "anthropic.claude-sonnet-4-5-20250929-v1:0"
        );
        assert_eq!(curated("google-gemini"), "gemini-2.5-pro");
        assert_eq!(curated("venice"), "zai-org-glm-5");
        assert_eq!(curated("moonshot"), "kimi-k2.5");
        assert_eq!(curated("nvidia"), "nvidia/meta/llama-3.3-70b-instruct");
        assert_eq!(curated("nvidia-nim"), "nvidia/meta/llama-3.3-70b-instruct");
        assert_eq!(curated("llamacpp"), "ggml-org/gpt-oss-20b-GGUF");
        assert_eq!(
            curated_default_model_for_provider("astrai"),
            DEFAULT_PROTOCOL_MODEL_ID
        );

        // Protocol provider/model ids pass through unchanged.
        assert_eq!(
            default_model_for_provider(DEFAULT_PROTOCOL_MODEL_ID),
            DEFAULT_PROTOCOL_MODEL_ID
        );
    }

    #[test]
    fn canonical_provider_name_normalizes_regional_aliases() {
        assert_eq!(canonical_provider_name("qwen-intl"), "qwen");
        assert_eq!(canonical_provider_name("dashscope-us"), "qwen");
        assert_eq!(canonical_provider_name("qwen-code"), "qwen-code");
        assert_eq!(canonical_provider_name("qwen-oauth"), "qwen-code");
        assert_eq!(canonical_provider_name("moonshot-intl"), "moonshot");
        assert_eq!(canonical_provider_name("kimi-cn"), "moonshot");
        assert_eq!(canonical_provider_name("kimi_coding"), "kimi-code");
        assert_eq!(canonical_provider_name("kimi_for_coding"), "kimi-code");
        assert_eq!(canonical_provider_name("glm-cn"), "glm");
        assert_eq!(canonical_provider_name("bigmodel"), "glm");
        assert_eq!(canonical_provider_name("minimax-cn"), "minimax");
        assert_eq!(canonical_provider_name("zai-cn"), "zai");
        assert_eq!(canonical_provider_name("z.ai-global"), "zai");
        assert_eq!(canonical_provider_name("nvidia-nim"), "nvidia");
        assert_eq!(canonical_provider_name("aws-bedrock"), "bedrock");
        assert_eq!(canonical_provider_name("build.nvidia.com"), "nvidia");
        assert_eq!(canonical_provider_name("llama.cpp"), "llamacpp");
    }

    #[test]
    fn curated_models_for_openai_include_latest_choices() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("openai"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"gpt-5.2".to_string()));
        assert!(ids.contains(&"gpt-5-mini".to_string()));
    }

    #[test]
    fn curated_models_for_glm_removes_deprecated_flash_plus_aliases() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("glm"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"glm-5".to_string()));
        assert!(ids.contains(&"glm-4.7".to_string()));
        assert!(ids.contains(&"glm-4.5-air".to_string()));
        assert!(!ids.contains(&"glm-4-plus".to_string()));
        assert!(!ids.contains(&"glm-4-flash".to_string()));
    }

    #[test]
    fn curated_models_for_openai_codex_include_codex_family() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("openai-codex"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"gpt-5-codex".to_string()));
        assert!(ids.contains(&"gpt-5.2-codex".to_string()));
    }

    #[test]
    fn curated_models_for_openrouter_use_valid_anthropic_id() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("openrouter"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"anthropic/claude-sonnet-4.6".to_string()));
    }

    #[test]
    fn curated_models_for_bedrock_include_verified_model_ids() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("bedrock"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"anthropic.claude-sonnet-4-6".to_string()));
        assert!(ids.contains(&"anthropic.claude-opus-4-6-v1".to_string()));
        assert!(ids.contains(&"anthropic.claude-haiku-4-5-20251001-v1:0".to_string()));
        assert!(ids.contains(&"anthropic.claude-sonnet-4-5-20250929-v1:0".to_string()));
    }

    #[test]
    fn curated_models_for_moonshot_drop_deprecated_aliases() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("moonshot"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"kimi-k2.5".to_string()));
        assert!(ids.contains(&"kimi-k2-thinking".to_string()));
        assert!(!ids.contains(&"kimi-latest".to_string()));
        assert!(!ids.contains(&"kimi-thinking-preview".to_string()));
    }

    #[test]
    fn allows_unauthenticated_model_fetch_for_public_catalogs() {
        assert!(allows_unauthenticated_model_fetch("openrouter"));
        assert!(allows_unauthenticated_model_fetch("venice"));
        assert!(allows_unauthenticated_model_fetch("nvidia"));
        assert!(allows_unauthenticated_model_fetch("nvidia-nim"));
        assert!(allows_unauthenticated_model_fetch("build.nvidia.com"));
        assert!(allows_unauthenticated_model_fetch("astrai"));
        assert!(allows_unauthenticated_model_fetch("ollama"));
        assert!(allows_unauthenticated_model_fetch("llamacpp"));
        assert!(allows_unauthenticated_model_fetch("llama.cpp"));
        assert!(!allows_unauthenticated_model_fetch("openai"));
        assert!(!allows_unauthenticated_model_fetch("deepseek"));
    }

    #[test]
    fn curated_models_for_kimi_code_include_official_agent_model() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("kimi-code"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"kimi-for-coding".to_string()));
        assert!(ids.contains(&"kimi-k2.5".to_string()));
    }

    #[test]
    fn curated_models_for_qwen_code_include_coding_plan_models() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("qwen-code"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"qwen3-coder-plus".to_string()));
        assert!(ids.contains(&"qwen3.5-plus".to_string()));
        assert!(ids.contains(&"qwen3-max-2026-01-23".to_string()));
    }

    #[test]
    fn supports_live_model_fetch_for_supported_and_unsupported_providers() {
        assert!(supports_live_model_fetch("openai"));
        assert!(supports_live_model_fetch("anthropic"));
        assert!(supports_live_model_fetch("gemini"));
        assert!(supports_live_model_fetch("google"));
        assert!(supports_live_model_fetch("grok"));
        assert!(supports_live_model_fetch("together"));
        assert!(supports_live_model_fetch("nvidia"));
        assert!(supports_live_model_fetch("nvidia-nim"));
        assert!(supports_live_model_fetch("build.nvidia.com"));
        assert!(supports_live_model_fetch("ollama"));
        assert!(supports_live_model_fetch("llamacpp"));
        assert!(supports_live_model_fetch("llama.cpp"));
        assert!(supports_live_model_fetch("astrai"));
        assert!(supports_live_model_fetch("venice"));
        assert!(supports_live_model_fetch("glm-cn"));
        assert!(supports_live_model_fetch("qwen-intl"));
        assert!(!supports_live_model_fetch("minimax-cn"));
        assert!(!supports_live_model_fetch("unknown-provider"));
    }

    #[test]
    fn curated_models_provider_aliases_share_same_catalog() {
        assert_eq!(
            curated_models_fallback(canonical_provider_name("xai")),
            curated_models_fallback(canonical_provider_name("grok"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("together-ai")),
            curated_models_fallback(canonical_provider_name("together"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("gemini")),
            curated_models_fallback(canonical_provider_name("google"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("gemini")),
            curated_models_fallback(canonical_provider_name("google-gemini"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("qwen")),
            curated_models_fallback(canonical_provider_name("qwen-intl"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("qwen")),
            curated_models_fallback(canonical_provider_name("dashscope-us"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("minimax")),
            curated_models_fallback(canonical_provider_name("minimax-cn"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("zai")),
            curated_models_fallback(canonical_provider_name("zai-cn"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("nvidia")),
            curated_models_fallback(canonical_provider_name("nvidia-nim"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("nvidia")),
            curated_models_fallback(canonical_provider_name("build.nvidia.com"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("llamacpp")),
            curated_models_fallback(canonical_provider_name("llama.cpp"))
        );
        assert_eq!(
            curated_models_fallback(canonical_provider_name("bedrock")),
            curated_models_fallback(canonical_provider_name("aws-bedrock"))
        );
    }

    #[test]
    fn curated_models_for_nvidia_include_nim_catalog_entries() {
        let ids: Vec<String> = curated_models_fallback(canonical_provider_name("nvidia"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert!(ids.contains(&"nvidia/meta/llama-3.3-70b-instruct".to_string()));
        assert!(ids.contains(&"nvidia/deepseek-ai/deepseek-v3.2".to_string()));
        assert!(ids.contains(&"nvidia/llama-3.3-nemotron-super-49b-v1.5".to_string()));
    }

    #[test]
    fn models_endpoint_for_provider_handles_region_aliases() {
        assert_eq!(
            models_endpoint_for_provider("glm-cn"),
            Some("https://open.bigmodel.cn/api/paas/v4/models")
        );
        assert_eq!(
            models_endpoint_for_provider("zai-cn"),
            Some("https://open.bigmodel.cn/api/coding/paas/v4/models")
        );
        assert_eq!(
            models_endpoint_for_provider("qwen-intl"),
            Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1/models")
        );
    }

    #[test]
    fn models_endpoint_for_provider_supports_additional_openai_compatible_providers() {
        assert_eq!(
            models_endpoint_for_provider("venice"),
            Some("https://api.venice.ai/api/v1/models")
        );
        assert_eq!(
            models_endpoint_for_provider("cohere"),
            Some("https://api.cohere.com/compatibility/v1/models")
        );
        assert_eq!(
            models_endpoint_for_provider("moonshot"),
            Some("https://api.moonshot.ai/v1/models")
        );
        assert_eq!(
            models_endpoint_for_provider("llamacpp"),
            Some("http://localhost:8080/v1/models")
        );
        assert_eq!(
            models_endpoint_for_provider("llama.cpp"),
            Some("http://localhost:8080/v1/models")
        );
        assert_eq!(models_endpoint_for_provider("perplexity"), None);
        assert_eq!(models_endpoint_for_provider("unknown-provider"), None);
    }

    #[test]
    fn resolve_live_models_endpoint_prefers_llamacpp_custom_url() {
        assert_eq!(
            resolve_live_models_endpoint("llamacpp", Some("http://127.0.0.1:8033/v1")),
            Some("http://127.0.0.1:8033/v1/models".to_string())
        );
        assert_eq!(
            resolve_live_models_endpoint("llama.cpp", Some("http://127.0.0.1:8033/v1/")),
            Some("http://127.0.0.1:8033/v1/models".to_string())
        );
        assert_eq!(
            resolve_live_models_endpoint("llamacpp", Some("http://127.0.0.1:8033/v1/models")),
            Some("http://127.0.0.1:8033/v1/models".to_string())
        );
    }

    #[test]
    fn resolve_live_models_endpoint_falls_back_to_provider_defaults() {
        assert_eq!(
            resolve_live_models_endpoint("llamacpp", None),
            Some("http://localhost:8080/v1/models".to_string())
        );
        assert_eq!(
            resolve_live_models_endpoint("venice", Some("http://localhost:9999/v1")),
            Some("https://api.venice.ai/api/v1/models".to_string())
        );
        assert_eq!(resolve_live_models_endpoint("unknown-provider", None), None);
    }

    #[test]
    fn normalize_ollama_endpoint_url_strips_api_suffix_and_trailing_slash() {
        assert_eq!(
            normalize_ollama_endpoint_url(" https://ollama.com/api/ "),
            "https://ollama.com".to_string()
        );
        assert_eq!(
            normalize_ollama_endpoint_url("https://ollama.com/"),
            "https://ollama.com".to_string()
        );
        assert_eq!(normalize_ollama_endpoint_url(""), "");
    }

    #[test]
    fn ollama_uses_remote_endpoint_distinguishes_local_and_remote_urls() {
        assert!(!ollama_uses_remote_endpoint(None));
        assert!(!ollama_uses_remote_endpoint(Some("http://localhost:11434")));
        assert!(!ollama_uses_remote_endpoint(Some(
            "http://127.0.0.1:11434/api"
        )));
        assert!(ollama_uses_remote_endpoint(Some("https://ollama.com")));
        assert!(ollama_uses_remote_endpoint(Some("https://ollama.com/api")));
    }

    #[test]
    fn parse_openai_model_ids_supports_data_array_payload() {
        let payload = json!({
            "data": [
                {"id": "  gpt-5.1  "},
                {"id": "gpt-5-mini"},
                {"id": "gpt-5.1"},
                {"id": ""}
            ]
        });

        let ids = parse_openai_compatible_model_ids(&payload);
        assert_eq!(ids, vec!["gpt-5-mini".to_string(), "gpt-5.1".to_string()]);
    }

    #[test]
    fn parse_openai_model_ids_supports_root_array_payload() {
        let payload = json!([
            {"id": "alpha"},
            {"id": "beta"},
            {"id": "alpha"}
        ]);

        let ids = parse_openai_compatible_model_ids(&payload);
        assert_eq!(ids, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn parse_gemini_model_ids_filters_for_generate_content() {
        let payload = json!({
            "models": [
                {
                    "name": "models/gemini-2.5-pro",
                    "supportedGenerationMethods": ["generateContent", "countTokens"]
                },
                {
                    "name": "models/text-embedding-004",
                    "supportedGenerationMethods": ["embedContent"]
                },
                {
                    "name": "models/gemini-2.5-flash",
                    "supportedGenerationMethods": ["generateContent"]
                }
            ]
        });

        let ids = parse_gemini_model_ids(&payload);
        assert_eq!(
            ids,
            vec!["gemini-2.5-flash".to_string(), "gemini-2.5-pro".to_string()]
        );
    }

    #[test]
    fn parse_ollama_model_ids_extracts_and_deduplicates_names() {
        let payload = json!({
            "models": [
                {"name": "llama3.2:latest"},
                {"name": "mistral:latest"},
                {"name": "llama3.2:latest"}
            ]
        });

        let ids = parse_ollama_model_ids(&payload);
        assert_eq!(
            ids,
            vec!["llama3.2:latest".to_string(), "mistral:latest".to_string()]
        );
    }

    #[test]
    fn model_cache_round_trip_returns_fresh_entry() {
        let tmp = TempDir::new().unwrap();
        let models = vec!["gpt-5.1".to_string(), "gpt-5-mini".to_string()];

        cache_live_models_for_provider(tmp.path(), "openai", &models).unwrap();

        let cached =
            load_cached_models_for_provider(tmp.path(), "openai", MODEL_CACHE_TTL_SECS).unwrap();
        let cached = cached.expect("expected fresh cached models");

        assert_eq!(cached.models.len(), 2);
        assert!(cached.models.contains(&"gpt-5.1".to_string()));
        assert!(cached.models.contains(&"gpt-5-mini".to_string()));
    }

    #[test]
    fn model_cache_ttl_filters_stale_entries() {
        let tmp = TempDir::new().unwrap();
        let stale = ModelCacheState {
            entries: vec![ModelCacheEntry {
                provider: "openai".to_string(),
                fetched_at_unix: now_unix_secs().saturating_sub(MODEL_CACHE_TTL_SECS + 120),
                models: vec!["gpt-5.1".to_string()],
            }],
        };

        save_model_cache_state(tmp.path(), &stale).unwrap();

        let fresh =
            load_cached_models_for_provider(tmp.path(), "openai", MODEL_CACHE_TTL_SECS).unwrap();
        assert!(fresh.is_none());

        let stale_any = load_any_cached_models_for_provider(tmp.path(), "openai").unwrap();
        assert!(stale_any.is_some());
    }

    #[test]
    fn run_models_refresh_uses_fresh_cache_without_network() {
        let tmp = TempDir::new().unwrap();

        cache_live_models_for_provider(tmp.path(), "openai", &["gpt-5.1".to_string()]).unwrap();

        let config = Config {
            workspace_dir: tmp.path().to_path_buf(),
            default_provider: Some("openai".to_string()),
            ..Config::default()
        };

        run_models_refresh(&config, None, false).unwrap();
    }

    #[test]
    fn run_models_refresh_rejects_unsupported_provider() {
        let tmp = TempDir::new().unwrap();

        let config = Config {
            workspace_dir: tmp.path().to_path_buf(),
            // Use a non-provider channel key to keep this test deterministic and offline.
            default_provider: Some("imessage".to_string()),
            ..Config::default()
        };

        let err = run_models_refresh(&config, None, true).unwrap_err();
        assert!(err
            .to_string()
            .contains("does not support live model discovery"));
    }

    // ── provider_env_var ────────────────────────────────────────

    #[test]
    fn provider_env_var_known_providers() {
        assert_eq!(provider_env_var("openrouter"), "OPENROUTER_API_KEY");
        assert_eq!(provider_env_var("anthropic"), "ANTHROPIC_API_KEY");
        assert_eq!(provider_env_var("openai"), "OPENAI_API_KEY");
        assert_eq!(provider_env_var("ollama"), "OLLAMA_API_KEY");
        assert_eq!(provider_env_var("llamacpp"), "LLAMACPP_API_KEY");
        assert_eq!(provider_env_var("llama.cpp"), "LLAMACPP_API_KEY");
        assert_eq!(provider_env_var("xai"), "XAI_API_KEY");
        assert_eq!(provider_env_var("grok"), "XAI_API_KEY"); // alias
        assert_eq!(provider_env_var("together"), "TOGETHER_API_KEY"); // alias
        assert_eq!(provider_env_var("together-ai"), "TOGETHER_API_KEY");
        assert_eq!(provider_env_var("google"), "GEMINI_API_KEY"); // alias
        assert_eq!(provider_env_var("google-gemini"), "GEMINI_API_KEY"); // alias
        assert_eq!(provider_env_var("gemini"), "GEMINI_API_KEY");
        assert_eq!(provider_env_var("qwen"), "DASHSCOPE_API_KEY");
        assert_eq!(provider_env_var("qwen-intl"), "DASHSCOPE_API_KEY");
        assert_eq!(provider_env_var("dashscope-us"), "DASHSCOPE_API_KEY");
        assert_eq!(provider_env_var("qwen-code"), "QWEN_OAUTH_TOKEN");
        assert_eq!(provider_env_var("qwen-oauth"), "QWEN_OAUTH_TOKEN");
        assert_eq!(provider_env_var("glm-cn"), "GLM_API_KEY");
        assert_eq!(provider_env_var("minimax-cn"), "MINIMAX_API_KEY");
        assert_eq!(provider_env_var("kimi-code"), "KIMI_CODE_API_KEY");
        assert_eq!(provider_env_var("kimi_coding"), "KIMI_CODE_API_KEY");
        assert_eq!(provider_env_var("kimi_for_coding"), "KIMI_CODE_API_KEY");
        assert_eq!(provider_env_var("minimax-oauth"), "MINIMAX_API_KEY");
        assert_eq!(provider_env_var("minimax-oauth-cn"), "MINIMAX_API_KEY");
        assert_eq!(provider_env_var("moonshot-intl"), "MOONSHOT_API_KEY");
        assert_eq!(provider_env_var("zai-cn"), "ZAI_API_KEY");
        assert_eq!(provider_env_var("nvidia"), "NVIDIA_API_KEY");
        assert_eq!(provider_env_var("nvidia-nim"), "NVIDIA_API_KEY"); // alias
        assert_eq!(provider_env_var("build.nvidia.com"), "NVIDIA_API_KEY"); // alias
        assert_eq!(provider_env_var("astrai"), "ASTRAI_API_KEY");
    }

    #[test]
    fn provider_supports_keyless_local_usage_for_local_providers() {
        assert!(provider_supports_keyless_local_usage("ollama"));
        assert!(provider_supports_keyless_local_usage("llamacpp"));
        assert!(provider_supports_keyless_local_usage("llama.cpp"));
        assert!(!provider_supports_keyless_local_usage("openai"));
    }

    #[test]
    fn provider_env_var_unknown_falls_back() {
        assert_eq!(provider_env_var("some-new-provider"), "API_KEY");
    }

    #[test]
    fn backend_key_from_choice_maps_supported_backends() {
        assert_eq!(backend_key_from_choice(0), "sqlite");
        assert_eq!(backend_key_from_choice(1), "lucid");
        assert_eq!(backend_key_from_choice(2), "markdown");
        assert_eq!(backend_key_from_choice(3), "none");
        assert_eq!(backend_key_from_choice(999), "sqlite");
    }

    #[test]
    fn memory_backend_profile_marks_lucid_as_optional_sqlite_backed() {
        let lucid = memory_backend_profile("lucid");
        assert!(lucid.auto_save_default);
        assert!(lucid.uses_sqlite_hygiene);
        assert!(lucid.sqlite_based);
        assert!(lucid.optional_dependency);

        let markdown = memory_backend_profile("markdown");
        assert!(markdown.auto_save_default);
        assert!(!markdown.uses_sqlite_hygiene);

        let none = memory_backend_profile("none");
        assert!(!none.auto_save_default);
        assert!(!none.uses_sqlite_hygiene);

        let custom = memory_backend_profile("custom-memory");
        assert!(custom.auto_save_default);
        assert!(!custom.uses_sqlite_hygiene);
    }

    #[test]
    fn memory_config_defaults_for_lucid_enable_sqlite_hygiene() {
        let config = memory_config_defaults_for_backend("lucid");
        assert_eq!(config.backend, "lucid");
        assert!(config.auto_save);
        assert!(config.hygiene_enabled);
        assert_eq!(config.archive_after_days, 7);
        assert_eq!(config.purge_after_days, 30);
        assert_eq!(config.embedding_cache_size, 10000);
    }

    #[test]
    fn memory_config_defaults_for_none_disable_sqlite_hygiene() {
        let config = memory_config_defaults_for_backend("none");
        assert_eq!(config.backend, "none");
        assert!(!config.auto_save);
        assert!(!config.hygiene_enabled);
        assert_eq!(config.archive_after_days, 0);
        assert_eq!(config.purge_after_days, 0);
        assert_eq!(config.embedding_cache_size, 0);
    }

    #[test]
    fn launchable_channels_include_mattermost_and_qq() {
        let mut channels = ChannelsConfig::default();
        assert!(!has_launchable_channels(&channels));

        channels.mattermost = Some(crate::config::schema::MattermostConfig {
            url: "https://mattermost.example.com".into(),
            bot_token: "token".into(),
            channel_id: Some("channel".into()),
            allowed_users: vec!["*".into()],
            thread_replies: Some(true),
            mention_only: Some(false),
        });
        assert!(has_launchable_channels(&channels));

        channels.mattermost = None;
        channels.qq = Some(crate::config::schema::QQConfig {
            app_id: "app-id".into(),
            app_secret: "app-secret".into(),
            allowed_users: vec!["*".into()],
        });
        assert!(has_launchable_channels(&channels));
    }
}
