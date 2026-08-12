//! Interactive onboard wizard steps (VL-REVIEW2-A1).

#[allow(clippy::wildcard_imports)]
use super::*;
// ── Step helpers ─────────────────────────────────────────────────

pub(crate) fn print_step(current: u8, total: u8, title: &str) {
    println!();
    println!(
        "  {} {}",
        style(format!("[{current}/{total}]")).cyan().bold(),
        style(title).white().bold()
    );
    println!("  {}", style("─".repeat(50)).dim());
}

pub(crate) fn print_bullet(text: &str) {
    println!("  {} {}", style("›").cyan(), text);
}

pub(crate) fn ensure_onboard_overwrite_allowed(config_path: &Path, force: bool) -> Result<()> {
    if !config_path.exists() {
        return Ok(());
    }

    if force {
        println!(
            "  {} Existing config detected at {}. Proceeding because --force was provided.",
            style("!").yellow().bold(),
            style(config_path.display()).yellow()
        );
        return Ok(());
    }

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!(
            "Refusing to overwrite existing config at {} in non-interactive mode. Re-run with --force if overwrite is intentional.",
            config_path.display()
        );
    }

    let confirmed = Confirm::new()
        .with_prompt(format!(
            "  Existing config found at {}. Re-running onboarding will overwrite config.toml and may create missing workspace files (including BOOTSTRAP.md). Continue?",
            config_path.display()
        ))
        .default(false)
        .interact()?;

    if !confirmed {
        bail!("Onboarding canceled: existing configuration was left unchanged.");
    }

    Ok(())
}

pub(crate) async fn persist_workspace_selection(config_path: &Path) -> Result<()> {
    let config_dir = config_path
        .parent()
        .context("Config path must have a parent directory")?;
    crate::config::schema::persist_active_workspace_config_dir(config_dir)
        .await
        .with_context(|| {
            format!(
                "Failed to persist active workspace selection for {}",
                config_dir.display()
            )
        })
}

// ── Step 1: Workspace ────────────────────────────────────────────

pub(crate) fn setup_workspace() -> Result<(PathBuf, PathBuf)> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("Could not find home directory")?;
    let default_dir = home.join(".velaclaw");

    print_bullet(&format!(
        "Default location: {}",
        style(default_dir.display()).green()
    ));

    let use_default = Confirm::new()
        .with_prompt("  Use default workspace location?")
        .default(true)
        .interact()?;

    let velaclaw_dir = if use_default {
        default_dir
    } else {
        let custom: String = Input::new()
            .with_prompt("  Enter workspace path")
            .interact_text()?;
        let expanded = shellexpand::tilde(&custom).to_string();
        PathBuf::from(expanded)
    };

    let workspace_dir = velaclaw_dir.join("workspace");
    let config_path = velaclaw_dir.join("config.toml");

    fs::create_dir_all(&workspace_dir).context("Failed to create workspace directory")?;

    println!(
        "  {} Workspace: {}",
        style("✓").green().bold(),
        style(workspace_dir.display()).green()
    );

    Ok((workspace_dir, config_path))
}

// ── Step 2: Provider & API Key ───────────────────────────────────

pub(crate) fn print_gateway_tier_manifest_hints() {
    println!();
    print_bullet(
        "Gateway vendors (Vercel AI, Cloudflare AI, Astrai, …) only resolve when matching YAML manifests exist under your AI_PROTOCOL_DIR checkout.",
    );
    print_bullet(
        "If upstream ai-protocol does not ship your gateway yet, add manifests locally (or use Custom) and enter that manifest's provider/model id.",
    );
    println!();
}

#[allow(clippy::too_many_lines)]
pub(crate) fn setup_provider(
    workspace_dir: &Path,
) -> Result<(String, String, String, Option<String>)> {
    // ── Tier selection ──
    let tiers = vec![
        "⭐ Recommended protocol models (OpenAI, Anthropic, Gemini, DeepSeek)",
        "⚡ Fast inference (Groq, Fireworks, Together AI, NVIDIA NIM)",
        "🌐 Gateway / proxy (Bedrock via ai-protocol — other gateways need manifests in AI_PROTOCOL_DIR)",
        "🔬 Specialized (Moonshot/Kimi, GLM/Zhipu, MiniMax, Qwen/DashScope, Qianfan, Z.AI, Synthetic, OpenCode Zen, Cohere)",
        "🏠 Local / private (Ollama, llama.cpp server — no API key needed)",
        "🔧 Custom — use your own ai-protocol manifest",
    ];

    let tier_idx = Select::new()
        .with_prompt("  Select provider category")
        .items(&tiers)
        .default(0)
        .interact()?;

    let providers: Vec<(&str, &str)> = match tier_idx {
        0 => vec![
            (
                DEFAULT_PROTOCOL_MODEL_ID,
                "NVIDIA Nemotron via ai-protocol (free tier default)",
            ),
            (
                "anthropic/claude-sonnet-4-5-20250929",
                "Anthropic Claude Sonnet via ai-protocol",
            ),
            ("google/gemini-2.5-pro", "Google Gemini via ai-protocol"),
            ("deepseek/deepseek-chat", "DeepSeek via ai-protocol"),
            ("qwen/qwen-plus", "Qwen via ai-protocol"),
            (
                "openai-codex",
                "OpenAI Codex (ChatGPT subscription OAuth, no API key)",
            ),
        ],
        1 => vec![
            ("groq/llama-3.3-70b-versatile", "Groq via ai-protocol"),
            (
                "fireworks/accounts/fireworks/models/llama-v3p3-70b-instruct",
                "Fireworks AI via ai-protocol",
            ),
            (
                "together/meta-llama/Llama-3.3-70B-Instruct-Turbo",
                "Together AI via ai-protocol",
            ),
            (
                "nvidia/meta/llama-3.3-70b-instruct",
                "NVIDIA NIM via ai-protocol",
            ),
        ],
        2 => {
            print_gateway_tier_manifest_hints();
            vec![(
                "bedrock/anthropic.claude-sonnet-4-5-20250929-v1:0",
                "Amazon Bedrock via ai-protocol",
            )]
        }
        3 => vec![
            ("moonshot/kimi-k2.5", "Moonshot/Kimi via ai-protocol"),
            ("glm/glm-5", "GLM/Zhipu via ai-protocol"),
            ("minimax/MiniMax-M2.5", "MiniMax via ai-protocol"),
            ("qianfan/ernie-4.0-turbo-8k", "Qianfan via ai-protocol"),
            ("zai/glm-5", "Z.AI via ai-protocol"),
            ("cohere/command-a-03-2025", "Cohere via ai-protocol"),
        ],
        4 => vec![
            ("ollama/llama3.2", "Ollama via ai-protocol"),
            (
                "llamacpp/ggml-org/gpt-oss-20b-GGUF",
                "llama.cpp server via ai-protocol",
            ),
        ],
        _ => vec![], // Custom — handled below
    };

    // ── Custom / BYOP flow ──
    if providers.is_empty() {
        println!();
        println!(
            "  {} {}",
            style("Custom Provider Setup").white().bold(),
            style("— ai-protocol manifest").dim()
        );
        print_bullet("Define your endpoint as an ai-protocol provider manifest first.");
        print_bullet("Then enter the logical model id exposed by that manifest.");
        println!();

        let provider_name: String = Input::new()
            .with_prompt("  Logical model id (provider/model, e.g. local-gateway/my-model)")
            .interact_text()?;
        let provider_name = provider_name.trim().to_string();
        if crate::providers::parse_protocol_provider_model(&provider_name).is_none() {
            anyhow::bail!(
                "Custom providers must use provider/model syntax backed by an ai-protocol manifest"
            );
        }

        let api_key: String = Input::new()
            .with_prompt("  API key (or Enter to skip if not needed)")
            .allow_empty(true)
            .interact_text()?;

        let model = provider_name.clone();

        println!(
            "  {} Provider: {} | Model: {}",
            style("✓").green().bold(),
            style(&provider_name).green(),
            style(&model).green()
        );

        return Ok((provider_name, api_key, model, None));
    }

    let provider_labels: Vec<&str> = providers.iter().map(|(_, label)| *label).collect();

    let provider_idx = Select::new()
        .with_prompt("  Select your AI provider")
        .items(&provider_labels)
        .default(0)
        .interact()?;

    let provider_name = providers[provider_idx].0;

    // ── API key / endpoint ──
    let mut provider_api_url: Option<String> = None;
    let api_key = if provider_name == "ollama" {
        let use_remote_ollama = Confirm::new()
            .with_prompt("  Use a remote Ollama endpoint (for example Ollama Cloud)?")
            .default(false)
            .interact()?;

        if use_remote_ollama {
            let raw_url: String = Input::new()
                .with_prompt("  Remote Ollama endpoint URL")
                .default("https://ollama.com".into())
                .interact_text()?;

            let normalized_url = normalize_ollama_endpoint_url(&raw_url);
            if normalized_url.is_empty() {
                anyhow::bail!("Remote Ollama endpoint URL cannot be empty.");
            }
            let parsed = reqwest::Url::parse(&normalized_url)
                .context("Remote Ollama endpoint URL must be a valid URL")?;
            if !matches!(parsed.scheme(), "http" | "https") {
                anyhow::bail!("Remote Ollama endpoint URL must use http:// or https://");
            }

            provider_api_url = Some(normalized_url.clone());

            print_bullet(&format!(
                "Remote endpoint configured: {}",
                style(&normalized_url).cyan()
            ));
            if raw_url.trim().trim_end_matches('/') != normalized_url {
                print_bullet("Normalized endpoint to base URL (removed trailing /api).");
            }
            print_bullet(&format!(
                "If you use cloud-only models, append {} to the model ID.",
                style(":cloud").yellow()
            ));

            let key: String = Input::new()
                .with_prompt("  API key for remote Ollama endpoint (or Enter to skip)")
                .allow_empty(true)
                .interact_text()?;

            if key.trim().is_empty() {
                print_bullet(&format!(
                    "No API key provided. Set {} later if required by your endpoint.",
                    style("OLLAMA_API_KEY").yellow()
                ));
            }

            key
        } else {
            print_bullet("Using local Ollama at http://localhost:11434 (no API key needed).");
            String::new()
        }
    } else if matches!(provider_name, "llamacpp" | "llama.cpp") {
        let raw_url: String = Input::new()
            .with_prompt("  llama.cpp server endpoint URL")
            .default("http://localhost:8080/v1".into())
            .interact_text()?;

        let normalized_url = raw_url.trim().trim_end_matches('/').to_string();
        if normalized_url.is_empty() {
            anyhow::bail!("llama.cpp endpoint URL cannot be empty.");
        }
        provider_api_url = Some(normalized_url.clone());

        print_bullet(&format!(
            "Using llama.cpp server endpoint: {}",
            style(&normalized_url).cyan()
        ));
        print_bullet("No API key needed unless your llama.cpp server is started with --api-key.");

        let key: String = Input::new()
            .with_prompt("  API key for llama.cpp server (or Enter to skip)")
            .allow_empty(true)
            .interact_text()?;

        if key.trim().is_empty() {
            print_bullet(&format!(
                "No API key provided. Set {} later only if your server requires authentication.",
                style("LLAMACPP_API_KEY").yellow()
            ));
        }

        key
    } else if canonical_provider_name(provider_name) == "gemini" {
        if std::env::var("GEMINI_API_KEY").is_ok() {
            print_bullet(&format!(
                "{} GEMINI_API_KEY environment variable detected!",
                style("✓").green().bold()
            ));
            String::new()
        } else {
            print_bullet("Get your API key at: https://aistudio.google.com/app/apikey");
            print_bullet("Gemini now resolves through ai-protocol manifests; credentials follow the manifest/env credential chain.");
            println!();

            Input::new()
                .with_prompt("  Paste your Gemini API key (or press Enter to skip)")
                .allow_empty(true)
                .interact_text()?
        }
    } else if canonical_provider_name(provider_name) == "anthropic" {
        if std::env::var("ANTHROPIC_OAUTH_TOKEN").is_ok() {
            print_bullet(&format!(
                "{} ANTHROPIC_OAUTH_TOKEN environment variable detected!",
                style("✓").green().bold()
            ));
            String::new()
        } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            print_bullet(&format!(
                "{} ANTHROPIC_API_KEY environment variable detected!",
                style("✓").green().bold()
            ));
            String::new()
        } else {
            print_bullet(&format!(
                "Get your API key at: {}",
                style("https://console.anthropic.com/settings/keys")
                    .cyan()
                    .underlined()
            ));
            print_bullet("Or run `claude setup-token` to get an OAuth setup-token.");
            println!();

            let key: String = Input::new()
                .with_prompt("  Paste your API key or setup-token (or press Enter to skip)")
                .allow_empty(true)
                .interact_text()?;

            if key.is_empty() {
                print_bullet(&format!(
                    "Skipped. Set {} or {} or edit config.toml later.",
                    style("ANTHROPIC_API_KEY").yellow(),
                    style("ANTHROPIC_OAUTH_TOKEN").yellow()
                ));
            }

            key
        }
    } else if canonical_provider_name(provider_name) == "qwen-code" {
        if std::env::var("QWEN_OAUTH_TOKEN").is_ok() {
            print_bullet(&format!(
                "{} QWEN_OAUTH_TOKEN environment variable detected!",
                style("✓").green().bold()
            ));
            "qwen-oauth".to_string()
        } else {
            print_bullet(
                "Qwen Code OAuth credentials are usually stored in ~/.qwen/oauth_creds.json.",
            );
            print_bullet(
                "Run `qwen` once and complete OAuth login to populate cached credentials.",
            );
            print_bullet("You can also set QWEN_OAUTH_TOKEN directly.");
            println!();

            let key: String = Input::new()
                .with_prompt(
                    "  Paste your Qwen OAuth token (or press Enter to auto-detect cached OAuth)",
                )
                .allow_empty(true)
                .interact_text()?;

            if key.trim().is_empty() {
                print_bullet(&format!(
                    "Using OAuth auto-detection. Set {} and optional {} if needed.",
                    style("QWEN_OAUTH_TOKEN").yellow(),
                    style("QWEN_OAUTH_RESOURCE_URL").yellow()
                ));
                "qwen-oauth".to_string()
            } else {
                key
            }
        }
    } else {
        let key_url = if is_moonshot_alias(provider_name)
            || canonical_provider_name(provider_name) == "kimi-code"
        {
            "https://platform.moonshot.cn/console/api-keys"
        } else if canonical_provider_name(provider_name) == "qwen-code" {
            "https://qwen.readthedocs.io/en/latest/getting_started/installation.html"
        } else if is_glm_cn_alias(provider_name) || is_zai_cn_alias(provider_name) {
            "https://open.bigmodel.cn/usercenter/proj-mgmt/apikeys"
        } else if is_glm_alias(provider_name) || is_zai_alias(provider_name) {
            "https://platform.z.ai/"
        } else if is_minimax_alias(provider_name) {
            "https://www.minimaxi.com/user-center/basic-information"
        } else if is_qwen_alias(provider_name) {
            "https://help.aliyun.com/zh/model-studio/developer-reference/get-api-key"
        } else if is_qianfan_alias(provider_name) {
            "https://cloud.baidu.com/doc/WENXINWORKSHOP/s/7lm0vxo78"
        } else {
            match provider_name {
                "openrouter" => "https://openrouter.ai/keys",
                "openai" => "https://platform.openai.com/api-keys",
                "venice" => "https://venice.ai/settings/api",
                "groq" => "https://console.groq.com/keys",
                "mistral" => "https://console.mistral.ai/api-keys",
                "deepseek" => "https://platform.deepseek.com/api_keys",
                "together-ai" => "https://api.together.xyz/settings/api-keys",
                "fireworks" => "https://fireworks.ai/account/api-keys",
                "perplexity" => "https://www.perplexity.ai/settings/api",
                "xai" => "https://console.x.ai",
                "cohere" => "https://dashboard.cohere.com/api-keys",
                "vercel" => "https://vercel.com/account/tokens",
                "cloudflare" => "https://dash.cloudflare.com/profile/api-tokens",
                "nvidia" | "nvidia-nim" | "build.nvidia.com" => "https://build.nvidia.com/",
                "bedrock" => "https://console.aws.amazon.com/iam",
                "gemini" => "https://aistudio.google.com/app/apikey",
                "astrai" => "https://as-trai.com",
                _ => "",
            }
        };

        println!();
        if matches!(provider_name, "bedrock" | "aws-bedrock") {
            // Bedrock uses AWS AKSK, not a single API key.
            print_bullet("Bedrock uses AWS credentials (not a single API key).");
            print_bullet(&format!(
                "Set {} and {} environment variables.",
                style("AWS_ACCESS_KEY_ID").yellow(),
                style("AWS_SECRET_ACCESS_KEY").yellow(),
            ));
            print_bullet(&format!(
                "Optionally set {} for the region (default: us-east-1).",
                style("AWS_REGION").yellow(),
            ));
            if !key_url.is_empty() {
                print_bullet(&format!(
                    "Manage IAM credentials at: {}",
                    style(key_url).cyan().underlined()
                ));
            }
            println!();
            String::new()
        } else {
            if !key_url.is_empty() {
                print_bullet(&format!(
                    "Get your API key at: {}",
                    style(key_url).cyan().underlined()
                ));
            }
            print_bullet("You can also set it later via env var or config file.");
            println!();

            let key: String = Input::new()
                .with_prompt("  Paste your API key (or press Enter to skip)")
                .allow_empty(true)
                .interact_text()?;

            if key.is_empty() {
                let env_var = provider_env_var(provider_name);
                print_bullet(&format!(
                    "Skipped. Set {} or edit config.toml later.",
                    style(env_var).yellow()
                ));
            }

            key
        }
    };

    // ── Model selection ──
    let canonical_provider = canonical_provider_name(provider_name);
    let mut model_options: Vec<(String, String)> = curated_models_for_provider(canonical_provider);

    let mut live_options: Option<Vec<(String, String)>> = None;

    if supports_live_model_fetch(provider_name) {
        let ollama_remote = canonical_provider == "ollama"
            && ollama_uses_remote_endpoint(provider_api_url.as_deref());
        let can_fetch_without_key =
            allows_unauthenticated_model_fetch(provider_name) && !ollama_remote;
        let has_api_key = !api_key.trim().is_empty()
            || ((canonical_provider != "ollama" || ollama_remote)
                && std::env::var(provider_env_var(provider_name))
                    .ok()
                    .is_some_and(|value| !value.trim().is_empty()))
            || (provider_name == "minimax"
                && std::env::var("MINIMAX_OAUTH_TOKEN")
                    .ok()
                    .is_some_and(|value| !value.trim().is_empty()));

        if canonical_provider == "ollama" && ollama_remote && !has_api_key {
            print_bullet(&format!(
                "Remote Ollama live-model refresh needs an API key ({}); using curated models.",
                style("OLLAMA_API_KEY").yellow()
            ));
        }

        if can_fetch_without_key || has_api_key {
            if let Some(cached) =
                load_cached_models_for_provider(workspace_dir, provider_name, MODEL_CACHE_TTL_SECS)?
            {
                let shown_count = cached.models.len().min(LIVE_MODEL_MAX_OPTIONS);
                print_bullet(&format!(
                    "Found cached models ({shown_count}) updated {} ago.",
                    humanize_age(cached.age_secs)
                ));

                live_options = Some(build_model_options(
                    cached
                        .models
                        .into_iter()
                        .take(LIVE_MODEL_MAX_OPTIONS)
                        .collect(),
                    "cached",
                ));
            }

            let should_fetch_now = Confirm::new()
                .with_prompt(if live_options.is_some() {
                    "  Refresh models from provider now?"
                } else {
                    "  Fetch latest models from provider now?"
                })
                .default(live_options.is_none())
                .interact()?;

            if should_fetch_now {
                match fetch_live_models_for_provider(
                    provider_name,
                    &api_key,
                    provider_api_url.as_deref(),
                ) {
                    Ok(live_model_ids) if !live_model_ids.is_empty() => {
                        cache_live_models_for_provider(
                            workspace_dir,
                            provider_name,
                            &live_model_ids,
                        )?;

                        let fetched_count = live_model_ids.len();
                        let shown_count = fetched_count.min(LIVE_MODEL_MAX_OPTIONS);
                        let shown_models: Vec<String> = live_model_ids
                            .into_iter()
                            .take(LIVE_MODEL_MAX_OPTIONS)
                            .collect();

                        if shown_count < fetched_count {
                            print_bullet(&format!(
                                "Fetched {fetched_count} models. Showing first {shown_count}."
                            ));
                        } else {
                            print_bullet(&format!("Fetched {shown_count} live models."));
                        }

                        live_options = Some(build_model_options(shown_models, "live"));
                    }
                    Ok(_) => {
                        print_bullet("Provider returned no models; using curated list.");
                    }
                    Err(error) => {
                        print_bullet(&format!(
                            "Live fetch failed ({}); using cached/curated list.",
                            style(error.to_string()).yellow()
                        ));

                        if live_options.is_none() {
                            if let Some(stale) =
                                load_any_cached_models_for_provider(workspace_dir, provider_name)?
                            {
                                print_bullet(&format!(
                                    "Loaded stale cache from {} ago.",
                                    humanize_age(stale.age_secs)
                                ));

                                live_options = Some(build_model_options(
                                    stale
                                        .models
                                        .into_iter()
                                        .take(LIVE_MODEL_MAX_OPTIONS)
                                        .collect(),
                                    "stale-cache",
                                ));
                            }
                        }
                    }
                }
            }
        } else {
            print_bullet("No API key detected, so using curated model list.");
            print_bullet("Tip: add an API key and rerun onboarding to fetch live models.");
        }
    }

    if let Some(live_model_options) = live_options {
        let source_options = vec![
            format!("Provider model list ({})", live_model_options.len()),
            format!("Curated starter list ({})", model_options.len()),
        ];

        let source_idx = Select::new()
            .with_prompt("  Model source")
            .items(&source_options)
            .default(0)
            .interact()?;

        if source_idx == 0 {
            model_options = live_model_options;
        }
    }

    if model_options.is_empty() {
        model_options.push((
            default_model_for_provider(provider_name),
            "Provider default model".to_string(),
        ));
    }

    model_options.push((
        CUSTOM_MODEL_SENTINEL.to_string(),
        "Custom model ID (type manually)".to_string(),
    ));

    let model_labels: Vec<String> = model_options
        .iter()
        .map(|(model_id, label)| format!("{label} — {}", style(model_id).dim()))
        .collect();

    let model_idx = Select::new()
        .with_prompt("  Select your default model")
        .items(&model_labels)
        .default(0)
        .interact()?;

    let selected_model = model_options[model_idx].0.clone();
    let model = if selected_model == CUSTOM_MODEL_SENTINEL {
        Input::new()
            .with_prompt("  Enter custom model ID")
            .default(default_model_for_provider(provider_name))
            .interact_text()?
    } else {
        selected_model
    };

    println!(
        "  {} Provider: {} | Model: {}",
        style("✓").green().bold(),
        style(provider_name).green(),
        style(&model).green()
    );

    Ok((provider_name.to_string(), api_key, model, provider_api_url))
}

/// Map provider name to its conventional env var
pub(crate) fn provider_env_var(name: &str) -> &'static str {
    if canonical_provider_name(name) == "qwen-code" {
        return "QWEN_OAUTH_TOKEN";
    }

    match canonical_provider_name(name) {
        "openrouter" => "OPENROUTER_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "ollama" => "OLLAMA_API_KEY",
        "llamacpp" => "LLAMACPP_API_KEY",
        "venice" => "VENICE_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "xai" => "XAI_API_KEY",
        "together-ai" => "TOGETHER_API_KEY",
        "fireworks" | "fireworks-ai" => "FIREWORKS_API_KEY",
        "perplexity" => "PERPLEXITY_API_KEY",
        "cohere" => "COHERE_API_KEY",
        "kimi-code" => "KIMI_CODE_API_KEY",
        "moonshot" => "MOONSHOT_API_KEY",
        "glm" => "GLM_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "qwen" => "DASHSCOPE_API_KEY",
        "qianfan" => "QIANFAN_API_KEY",
        "zai" => "ZAI_API_KEY",
        "synthetic" => "SYNTHETIC_API_KEY",
        "opencode" | "opencode-zen" => "OPENCODE_API_KEY",
        "vercel" | "vercel-ai" => "VERCEL_API_KEY",
        "cloudflare" | "cloudflare-ai" => "CLOUDFLARE_API_KEY",
        "bedrock" | "aws-bedrock" => "AWS_ACCESS_KEY_ID",
        "gemini" => "GEMINI_API_KEY",
        "nvidia" | "nvidia-nim" | "build.nvidia.com" => "NVIDIA_API_KEY",
        "astrai" => "ASTRAI_API_KEY",
        _ => "API_KEY",
    }
}

pub(crate) fn provider_supports_keyless_local_usage(provider_name: &str) -> bool {
    matches!(
        canonical_provider_name(provider_name),
        "ollama" | "llamacpp"
    )
}

// ── Step 5: Tool Mode & Security ────────────────────────────────

pub(crate) fn setup_tool_mode() -> Result<(ComposioConfig, SecretsConfig)> {
    print_bullet("Choose how VelaClaw connects to external apps.");
    print_bullet("You can always change this later in config.toml.");
    println!();

    let options = vec![
        "Sovereign (local only) — you manage API keys, full privacy (default)",
        "Composio (managed OAuth) — 1000+ apps via OAuth, no raw keys shared",
    ];

    let choice = Select::new()
        .with_prompt("  Select tool mode")
        .items(&options)
        .default(0)
        .interact()?;

    let composio_config = if choice == 1 {
        println!();
        println!(
            "  {} {}",
            style("Composio Setup").white().bold(),
            style("— 1000+ OAuth integrations (Gmail, Notion, GitHub, Slack, ...)").dim()
        );
        print_bullet("Get your API key at: https://app.composio.dev/settings");
        print_bullet("VelaClaw uses Composio as a tool — your core agent stays local.");
        println!();

        let api_key: String = Input::new()
            .with_prompt("  Composio API key (or Enter to skip)")
            .allow_empty(true)
            .interact_text()?;

        if api_key.trim().is_empty() {
            println!(
                "  {} Skipped — set composio.api_key in config.toml later",
                style("→").dim()
            );
            ComposioConfig::default()
        } else {
            println!(
                "  {} Composio: {} (1000+ OAuth tools available)",
                style("✓").green().bold(),
                style("enabled").green()
            );
            ComposioConfig {
                enabled: true,
                api_key: Some(api_key),
                ..ComposioConfig::default()
            }
        }
    } else {
        println!(
            "  {} Tool mode: {} — full privacy, you own every key",
            style("✓").green().bold(),
            style("Sovereign (local only)").green()
        );
        ComposioConfig::default()
    };

    // ── Encrypted secrets ──
    println!();
    print_bullet("VelaClaw can encrypt API keys stored in config.toml.");
    print_bullet("A local key file protects against plaintext exposure and accidental leaks.");

    let encrypt = Confirm::new()
        .with_prompt("  Enable encrypted secret storage?")
        .default(true)
        .interact()?;

    let secrets_config = SecretsConfig { encrypt };

    if encrypt {
        println!(
            "  {} Secrets: {} — keys encrypted with local key file",
            style("✓").green().bold(),
            style("encrypted").green()
        );
    } else {
        println!(
            "  {} Secrets: {} — keys stored as plaintext (not recommended)",
            style("✓").green().bold(),
            style("plaintext").yellow()
        );
    }

    Ok((composio_config, secrets_config))
}

// ── Step 6: Hardware (Physical World) ───────────────────────────

pub(crate) fn setup_hardware() -> Result<HardwareConfig> {
    print_bullet("VelaClaw can talk to physical hardware (LEDs, sensors, motors).");
    print_bullet("Scanning for connected devices...");
    println!();

    // ── Auto-discovery ──
    let devices = hardware::discover_hardware();

    if devices.is_empty() {
        println!(
            "  {} {}",
            style("ℹ").dim(),
            style("No hardware devices detected on this system.").dim()
        );
        println!(
            "  {} {}",
            style("ℹ").dim(),
            style("You can enable hardware later in config.toml under [hardware].").dim()
        );
    } else {
        println!(
            "  {} {} device(s) found:",
            style("✓").green().bold(),
            devices.len()
        );
        for device in &devices {
            let detail = device
                .detail
                .as_deref()
                .map(|d| format!(" ({d})"))
                .unwrap_or_default();
            let path = device
                .device_path
                .as_deref()
                .map(|p| format!(" → {p}"))
                .unwrap_or_default();
            println!(
                "    {} {}{}{} [{}]",
                style("›").cyan(),
                style(&device.name).green(),
                style(&detail).dim(),
                style(&path).dim(),
                style(device.transport.to_string()).cyan()
            );
        }
    }
    println!();

    let options = vec![
        "🚀 Native — direct GPIO on this Linux board (Raspberry Pi, Orange Pi, etc.)",
        "🔌 Tethered — control an Arduino/ESP32/Nucleo plugged into USB",
        "🔬 Debug Probe — flash/read MCUs via SWD/JTAG (probe-rs)",
        "☁️  Software Only — no hardware access (default)",
    ];

    let recommended = hardware::recommended_wizard_default(&devices);

    let choice = Select::new()
        .with_prompt("  How should VelaClaw interact with the physical world?")
        .items(&options)
        .default(recommended)
        .interact()?;

    let mut hw_config = hardware::config_from_wizard_choice(choice, &devices);

    // ── Serial: pick a port if multiple found ──
    if hw_config.transport_mode() == hardware::HardwareTransport::Serial {
        let serial_devices: Vec<&hardware::DiscoveredDevice> = devices
            .iter()
            .filter(|d| d.transport == hardware::HardwareTransport::Serial)
            .collect();

        if serial_devices.len() > 1 {
            let port_labels: Vec<String> = serial_devices
                .iter()
                .map(|d| {
                    format!(
                        "{} ({})",
                        d.device_path.as_deref().unwrap_or("unknown"),
                        d.name
                    )
                })
                .collect();

            let port_idx = Select::new()
                .with_prompt("  Multiple serial devices found — select one")
                .items(&port_labels)
                .default(0)
                .interact()?;

            hw_config.serial_port = serial_devices[port_idx].device_path.clone();
        } else if serial_devices.is_empty() {
            // User chose serial but no device discovered — ask for manual path
            let manual_port: String = Input::new()
                .with_prompt("  Serial port path (e.g. /dev/ttyUSB0)")
                .default("/dev/ttyUSB0".into())
                .interact_text()?;
            hw_config.serial_port = Some(manual_port);
        }

        // Baud rate
        let baud_options = vec![
            "115200 (default, recommended)",
            "9600 (legacy Arduino)",
            "57600",
            "230400",
            "Custom",
        ];
        let baud_idx = Select::new()
            .with_prompt("  Serial baud rate")
            .items(&baud_options)
            .default(0)
            .interact()?;

        hw_config.baud_rate = match baud_idx {
            1 => 9600,
            2 => 57600,
            3 => 230_400,
            4 => {
                let custom: String = Input::new()
                    .with_prompt("  Custom baud rate")
                    .default("115200".into())
                    .interact_text()?;
                custom.parse::<u32>().unwrap_or(115_200)
            }
            _ => 115_200,
        };
    }

    // ── Probe: ask for target chip ──
    if hw_config.transport_mode() == hardware::HardwareTransport::Probe
        && hw_config.probe_target.is_none()
    {
        let target: String = Input::new()
            .with_prompt("  Target MCU chip (e.g. STM32F411CEUx, nRF52840_xxAA)")
            .default("STM32F411CEUx".into())
            .interact_text()?;
        hw_config.probe_target = Some(target);
    }

    // ── Datasheet RAG ──
    if hw_config.enabled {
        let datasheets = Confirm::new()
            .with_prompt("  Enable datasheet RAG? (index PDF schematics for AI pin lookups)")
            .default(true)
            .interact()?;
        hw_config.workspace_datasheets = datasheets;
    }

    // ── Summary ──
    if hw_config.enabled {
        let transport_label = match hw_config.transport_mode() {
            hardware::HardwareTransport::Native => "Native GPIO".to_string(),
            hardware::HardwareTransport::Serial => format!(
                "Serial → {} @ {} baud",
                hw_config.serial_port.as_deref().unwrap_or("?"),
                hw_config.baud_rate
            ),
            hardware::HardwareTransport::Probe => format!(
                "Probe (SWD/JTAG) → {}",
                hw_config.probe_target.as_deref().unwrap_or("?")
            ),
            hardware::HardwareTransport::None => "Software Only".to_string(),
        };

        println!(
            "  {} Hardware: {} | datasheets: {}",
            style("✓").green().bold(),
            style(&transport_label).green(),
            if hw_config.workspace_datasheets {
                style("on").green().to_string()
            } else {
                style("off").dim().to_string()
            }
        );
    } else {
        println!(
            "  {} Hardware: {}",
            style("✓").green().bold(),
            style("disabled (software only)").dim()
        );
    }

    Ok(hw_config)
}

// ── Step 6: Project Context ─────────────────────────────────────

pub(crate) fn setup_project_context() -> Result<ProjectContext> {
    print_bullet("Let's personalize your agent. You can always update these later.");
    print_bullet("Press Enter to accept defaults.");
    println!();

    let user_name: String = Input::new()
        .with_prompt("  Your name")
        .default("User".into())
        .interact_text()?;

    let tz_options = vec![
        "US/Eastern (EST/EDT)",
        "US/Central (CST/CDT)",
        "US/Mountain (MST/MDT)",
        "US/Pacific (PST/PDT)",
        "Europe/London (GMT/BST)",
        "Europe/Berlin (CET/CEST)",
        "Asia/Tokyo (JST)",
        "UTC",
        "Other (type manually)",
    ];

    let tz_idx = Select::new()
        .with_prompt("  Your timezone")
        .items(&tz_options)
        .default(0)
        .interact()?;

    let timezone = if tz_idx == tz_options.len() - 1 {
        Input::new()
            .with_prompt("  Enter timezone (e.g. America/New_York)")
            .default("UTC".into())
            .interact_text()?
    } else {
        // Extract the short label before the parenthetical
        tz_options[tz_idx]
            .split('(')
            .next()
            .unwrap_or("UTC")
            .trim()
            .to_string()
    };

    let agent_name: String = Input::new()
        .with_prompt("  Agent name")
        .default("VelaClaw".into())
        .interact_text()?;

    let style_options = vec![
        "Direct & concise — skip pleasantries, get to the point",
        "Friendly & casual — warm, human, and helpful",
        "Professional & polished — calm, confident, and clear",
        "Expressive & playful — more personality + natural emojis",
        "Technical & detailed — thorough explanations, code-first",
        "Balanced — adapt to the situation",
        "Custom — write your own style guide",
    ];

    let style_idx = Select::new()
        .with_prompt("  Communication style")
        .items(&style_options)
        .default(1)
        .interact()?;

    let communication_style = match style_idx {
        0 => "Be direct and concise. Skip pleasantries. Get to the point.".to_string(),
        1 => "Be friendly, human, and conversational. Show warmth and empathy while staying efficient. Use natural contractions.".to_string(),
        2 => "Be professional and polished. Stay calm, structured, and respectful. Use occasional tone-setting emojis only when appropriate.".to_string(),
        3 => "Be expressive and playful when appropriate. Use relevant emojis naturally (0-2 max), and keep serious topics emoji-light.".to_string(),
        4 => "Be technical and detailed. Thorough explanations, code-first.".to_string(),
        5 => "Adapt to the situation. Default to warm and clear communication; be concise when needed, thorough when it matters.".to_string(),
        _ => Input::new()
            .with_prompt("  Custom communication style")
            .default(
                "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing.".into(),
            )
            .interact_text()?,
    };

    println!(
        "  {} Context: {} | {} | {} | {}",
        style("✓").green().bold(),
        style(&user_name).green(),
        style(&timezone).green(),
        style(&agent_name).green(),
        style(&communication_style).green().dim()
    );

    Ok(ProjectContext {
        user_name,
        timezone,
        agent_name,
        communication_style,
    })
}

// ── Step 6: Memory Configuration ───────────────────────────────

pub(crate) fn setup_memory() -> Result<MemoryConfig> {
    print_bullet("Choose how VelaClaw stores and searches memories.");
    print_bullet("You can always change this later in config.toml.");
    println!();

    let options: Vec<&str> = selectable_memory_backends()
        .iter()
        .map(|backend| backend.label)
        .collect();

    let choice = Select::new()
        .with_prompt("  Select memory backend")
        .items(&options)
        .default(0)
        .interact()?;

    let backend = backend_key_from_choice(choice);
    let profile = memory_backend_profile(backend);

    let auto_save = profile.auto_save_default
        && Confirm::new()
            .with_prompt("  Auto-save conversations to memory?")
            .default(true)
            .interact()?;

    println!(
        "  {} Memory: {} (auto-save: {})",
        style("✓").green().bold(),
        style(backend).green(),
        if auto_save { "on" } else { "off" }
    );

    let mut config = memory_config_defaults_for_backend(backend);
    config.auto_save = auto_save;
    Ok(config)
}

// ── Step 3: Channels ────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub(crate) fn setup_channels() -> Result<ChannelsConfig> {
    print_bullet("Channels let you talk to VelaClaw from anywhere.");
    print_bullet("CLI is always available. Connect more channels now.");
    println!();

    let mut config = ChannelsConfig::default();
    #[derive(Clone, Copy)]
    enum ChannelMenuChoice {
        Telegram,
        Discord,
        Slack,
        IMessage,
        Matrix,
        WhatsApp,
        Linq,
        Irc,
        Webhook,
        DingTalk,
        QqOfficial,
        LarkFeishu,
        Done,
    }
    let menu_choices = [
        ChannelMenuChoice::Telegram,
        ChannelMenuChoice::Discord,
        ChannelMenuChoice::Slack,
        ChannelMenuChoice::IMessage,
        ChannelMenuChoice::Matrix,
        ChannelMenuChoice::WhatsApp,
        ChannelMenuChoice::Linq,
        ChannelMenuChoice::Irc,
        ChannelMenuChoice::Webhook,
        ChannelMenuChoice::DingTalk,
        ChannelMenuChoice::QqOfficial,
        ChannelMenuChoice::LarkFeishu,
        ChannelMenuChoice::Done,
    ];

    loop {
        let options: Vec<String> = menu_choices
            .iter()
            .map(|choice| match choice {
                ChannelMenuChoice::Telegram => format!(
                    "Telegram   {}",
                    if config.telegram.is_some() {
                        "✅ connected"
                    } else {
                        "— connect your bot"
                    }
                ),
                ChannelMenuChoice::Discord => format!(
                    "Discord    {}",
                    if config.discord.is_some() {
                        "✅ connected"
                    } else {
                        "— connect your bot"
                    }
                ),
                ChannelMenuChoice::Slack => format!(
                    "Slack      {}",
                    if config.slack.is_some() {
                        "✅ connected"
                    } else {
                        "— connect your bot"
                    }
                ),
                ChannelMenuChoice::IMessage => format!(
                    "iMessage   {}",
                    if config.imessage.is_some() {
                        "✅ configured"
                    } else {
                        "— macOS only"
                    }
                ),
                ChannelMenuChoice::Matrix => format!(
                    "Matrix     {}",
                    if config.matrix.is_some() {
                        "✅ connected"
                    } else {
                        "— self-hosted chat"
                    }
                ),
                ChannelMenuChoice::WhatsApp => format!(
                    "WhatsApp   {}",
                    if config.whatsapp.is_some() {
                        "✅ connected"
                    } else {
                        "— Business Cloud API"
                    }
                ),
                ChannelMenuChoice::Linq => format!(
                    "Linq       {}",
                    if config.linq.is_some() {
                        "✅ connected"
                    } else {
                        "— iMessage/RCS/SMS via Linq API"
                    }
                ),
                ChannelMenuChoice::Irc => format!(
                    "IRC        {}",
                    if config.irc.is_some() {
                        "✅ configured"
                    } else {
                        "— IRC over TLS"
                    }
                ),
                ChannelMenuChoice::Webhook => format!(
                    "Webhook    {}",
                    if config.webhook.is_some() {
                        "✅ configured"
                    } else {
                        "— HTTP endpoint"
                    }
                ),
                ChannelMenuChoice::DingTalk => format!(
                    "DingTalk   {}",
                    if config.dingtalk.is_some() {
                        "✅ connected"
                    } else {
                        "— DingTalk Stream Mode"
                    }
                ),
                ChannelMenuChoice::QqOfficial => format!(
                    "QQ Official {}",
                    if config.qq.is_some() {
                        "✅ connected"
                    } else {
                        "— Tencent QQ Bot"
                    }
                ),
                ChannelMenuChoice::LarkFeishu => format!(
                    "Lark/Feishu {}",
                    if config.lark.is_some() {
                        "✅ connected"
                    } else {
                        "— Lark/Feishu Bot"
                    }
                ),
                ChannelMenuChoice::Done => "Done — finish setup".to_string(),
            })
            .collect();

        let selection = Select::new()
            .with_prompt("  Connect a channel (or Done to continue)")
            .items(&options)
            .default(options.len() - 1)
            .interact()?;

        let choice = menu_choices
            .get(selection)
            .copied()
            .unwrap_or(ChannelMenuChoice::Done);

        match choice {
            ChannelMenuChoice::Telegram => {
                // ── Telegram ──
                println!();
                println!(
                    "  {} {}",
                    style("Telegram Setup").white().bold(),
                    style("— talk to VelaClaw from Telegram").dim()
                );
                print_bullet("1. Open Telegram and message @BotFather");
                print_bullet("2. Send /newbot and follow the prompts");
                print_bullet("3. Copy the bot token and paste it below");
                println!();

                let token: String = Input::new()
                    .with_prompt("  Bot token (from @BotFather)")
                    .interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection (run entirely in separate thread — reqwest::blocking Response
                // must be used and dropped there to avoid "Cannot drop a runtime" panic)
                print!("  {} Testing connection... ", style("⏳").dim());
                let token_clone = token.clone();
                let thread_result = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    let url = format!("https://api.telegram.org/bot{token_clone}/getMe");
                    let resp = client.get(&url).send()?;
                    let ok = resp.status().is_success();
                    let data: serde_json::Value = resp.json().unwrap_or_default();
                    let bot_name = data
                        .get("result")
                        .and_then(|r| r.get("username"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    Ok::<_, reqwest::Error>((ok, bot_name))
                })
                .join();
                match thread_result {
                    Ok(Ok((true, bot_name))) => {
                        println!(
                            "\r  {} Connected as @{bot_name}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token and try again",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                print_bullet(
                    "Allowlist your own Telegram identity first (recommended for secure + fast setup).",
                );
                print_bullet(
                    "Use your @username without '@' (example: argenis), or your numeric Telegram user ID.",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Telegram identities (comma-separated: username without '@' and/or numeric user ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Telegram inbound messages will be denied until you add your username/user ID or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.telegram = Some(TelegramConfig {
                    bot_token: token,
                    allowed_users,
                    stream_mode: StreamMode::default(),
                    draft_update_interval_ms: 1000,
                    interrupt_on_new_message: false,
                    mention_only: false,
                    approval_mode: ChannelApprovalMode::default(),
                    approval_timeout_secs: 300,
                });
            }
            ChannelMenuChoice::Discord => {
                // ── Discord ──
                println!();
                println!(
                    "  {} {}",
                    style("Discord Setup").white().bold(),
                    style("— talk to VelaClaw from Discord").dim()
                );
                print_bullet("1. Go to https://discord.com/developers/applications");
                print_bullet("2. Create a New Application → Bot → Copy token");
                print_bullet("3. Enable MESSAGE CONTENT intent under Bot settings");
                print_bullet("4. Invite bot to your server with messages permission");
                println!();

                let token: String = Input::new().with_prompt("  Bot token").interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection (run entirely in separate thread — Response must be used/dropped there)
                print!("  {} Testing connection... ", style("⏳").dim());
                let token_clone = token.clone();
                let thread_result = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    let resp = client
                        .get("https://discord.com/api/v10/users/@me")
                        .header("Authorization", format!("Bot {token_clone}"))
                        .send()?;
                    let ok = resp.status().is_success();
                    let data: serde_json::Value = resp.json().unwrap_or_default();
                    let bot_name = data
                        .get("username")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    Ok::<_, reqwest::Error>((ok, bot_name))
                })
                .join();
                match thread_result {
                    Ok(Ok((true, bot_name))) => {
                        println!(
                            "\r  {} Connected as {bot_name}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token and try again",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let guild: String = Input::new()
                    .with_prompt("  Server (guild) ID (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                print_bullet("Allowlist your own Discord user ID first (recommended).");
                print_bullet(
                    "Get it in Discord: Settings -> Advanced -> Developer Mode (ON), then right-click your profile -> Copy User ID.",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let allowed_users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Discord user IDs (comma-separated, recommended: your own ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim().is_empty() {
                    vec![]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Discord inbound messages will be denied until you add IDs or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.discord = Some(DiscordConfig {
                    bot_token: token,
                    guild_id: if guild.is_empty() { None } else { Some(guild) },
                    allowed_users,
                    listen_to_bots: false,
                    mention_only: false,
                });
            }
            ChannelMenuChoice::Slack => {
                // ── Slack ──
                println!();
                println!(
                    "  {} {}",
                    style("Slack Setup").white().bold(),
                    style("— talk to VelaClaw from Slack").dim()
                );
                print_bullet("1. Go to https://api.slack.com/apps → Create New App");
                print_bullet("2. Add Bot Token Scopes: chat:write, channels:history");
                print_bullet("3. Install to workspace and copy the Bot Token");
                println!();

                let token: String = Input::new()
                    .with_prompt("  Bot token (xoxb-...)")
                    .interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection (run entirely in separate thread — Response must be used/dropped there)
                print!("  {} Testing connection... ", style("⏳").dim());
                let token_clone = token.clone();
                let thread_result = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    let resp = client
                        .get("https://slack.com/api/auth.test")
                        .bearer_auth(&token_clone)
                        .send()?;
                    let ok = resp.status().is_success();
                    let data: serde_json::Value = resp.json().unwrap_or_default();
                    let api_ok = data
                        .get("ok")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let team = data
                        .get("team")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    let err = data
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown error")
                        .to_string();
                    Ok::<_, reqwest::Error>((ok, api_ok, team, err))
                })
                .join();
                match thread_result {
                    Ok(Ok((true, true, team, _))) => {
                        println!(
                            "\r  {} Connected to workspace: {team}        ",
                            style("✅").green().bold()
                        );
                    }
                    Ok(Ok((true, false, _, err))) => {
                        println!("\r  {} Slack error: {err}", style("❌").red().bold());
                        continue;
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let app_token: String = Input::new()
                    .with_prompt("  App token (xapp-..., optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                let channel: String = Input::new()
                    .with_prompt("  Default channel ID (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                print_bullet("Allowlist your own Slack member ID first (recommended).");
                print_bullet(
                    "Member IDs usually start with 'U' (open your Slack profile -> More -> Copy member ID).",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let allowed_users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Slack user IDs (comma-separated, recommended: your own member ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim().is_empty() {
                    vec![]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Slack inbound messages will be denied until you add IDs or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.slack = Some(SlackConfig {
                    bot_token: token,
                    app_token: if app_token.is_empty() {
                        None
                    } else {
                        Some(app_token)
                    },
                    channel_id: if channel.is_empty() {
                        None
                    } else {
                        Some(channel)
                    },
                    allowed_users,
                });
            }
            ChannelMenuChoice::IMessage => {
                // ── iMessage ──
                println!();
                println!(
                    "  {} {}",
                    style("iMessage Setup").white().bold(),
                    style("— macOS only, reads from Messages.app").dim()
                );

                if !cfg!(target_os = "macos") {
                    println!(
                        "  {} iMessage is only available on macOS.",
                        style("⚠").yellow().bold()
                    );
                    continue;
                }

                print_bullet("VelaClaw reads your iMessage database and replies via AppleScript.");
                print_bullet(
                    "You need to grant Full Disk Access to your terminal in System Settings.",
                );
                println!();

                let contacts_str: String = Input::new()
                    .with_prompt("  Allowed contacts (comma-separated phone/email, or * for all)")
                    .default("*".into())
                    .interact_text()?;

                let allowed_contacts = if contacts_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    contacts_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect()
                };

                config.imessage = Some(IMessageConfig { allowed_contacts });
                println!(
                    "  {} iMessage configured (contacts: {})",
                    style("✅").green().bold(),
                    style(&contacts_str).cyan()
                );
            }
            ChannelMenuChoice::Matrix => {
                // ── Matrix ──
                println!();
                println!(
                    "  {} {}",
                    style("Matrix Setup").white().bold(),
                    style("— self-hosted, federated chat").dim()
                );
                print_bullet("You need a Matrix account and an access token.");
                print_bullet("Get a token via Element → Settings → Help & About → Access Token.");
                println!();

                let homeserver: String = Input::new()
                    .with_prompt("  Homeserver URL (e.g. https://matrix.org)")
                    .interact_text()?;

                if homeserver.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let access_token: String =
                    Input::new().with_prompt("  Access token").interact_text()?;

                if access_token.trim().is_empty() {
                    println!("  {} Skipped — token required", style("→").dim());
                    continue;
                }

                // Test connection (run entirely in separate thread — Response must be used/dropped there)
                let hs = homeserver.trim_end_matches('/');
                print!("  {} Testing connection... ", style("⏳").dim());
                let hs_owned = hs.to_string();
                let access_token_clone = access_token.clone();
                let thread_result = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    let resp = client
                        .get(format!("{hs_owned}/_matrix/client/v3/account/whoami"))
                        .header("Authorization", format!("Bearer {access_token_clone}"))
                        .send()?;
                    let ok = resp.status().is_success();

                    if !ok {
                        return Ok::<_, reqwest::Error>((false, None, None));
                    }

                    let payload: Value = match resp.json() {
                        Ok(payload) => payload,
                        Err(_) => Value::Null,
                    };
                    let user_id = payload
                        .get("user_id")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string());
                    let device_id = payload
                        .get("device_id")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string());

                    Ok::<_, reqwest::Error>((true, user_id, device_id))
                })
                .join();

                let (detected_user_id, detected_device_id) = match thread_result {
                    Ok(Ok((true, user_id, device_id))) => {
                        println!(
                            "\r  {} Connection verified        ",
                            style("✅").green().bold()
                        );

                        if device_id.is_none() {
                            println!(
                                "  {} Homeserver did not return device_id from whoami. If E2EE decryption fails, set channels.matrix.device_id manually in config.toml.",
                                style("⚠️").yellow().bold()
                            );
                        }

                        (user_id, device_id)
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check homeserver URL and token",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                };

                let room_id: String = Input::new()
                    .with_prompt("  Room ID (e.g. !abc123:matrix.org)")
                    .interact_text()?;

                let users_str: String = Input::new()
                    .with_prompt("  Allowed users (comma-separated @user:server, or * for all)")
                    .default("*".into())
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                config.matrix = Some(MatrixConfig {
                    homeserver: homeserver.trim_end_matches('/').to_string(),
                    access_token,
                    user_id: detected_user_id,
                    device_id: detected_device_id,
                    room_id,
                    allowed_users,
                });
            }
            ChannelMenuChoice::WhatsApp => {
                // ── WhatsApp ──
                println!();
                println!("  {}", style("WhatsApp Setup").white().bold());

                let mode_options = vec![
                    "WhatsApp Web (QR / pair-code, no Meta Business API)",
                    "WhatsApp Business Cloud API (webhook)",
                ];
                let mode_idx = Select::new()
                    .with_prompt("  Choose WhatsApp mode")
                    .items(&mode_options)
                    .default(0)
                    .interact()?;

                if mode_idx == 0 {
                    println!("  {}", style("Mode: WhatsApp Web").dim());
                    print_bullet("1. Build with --features whatsapp-web");
                    print_bullet(
                        "2. Start channel/daemon and scan QR in WhatsApp > Linked Devices",
                    );
                    print_bullet("3. Keep session_path persistent so relogin is not required");
                    println!();

                    let session_path: String = Input::new()
                        .with_prompt("  Session database path")
                        .default("~/.velaclaw/state/whatsapp-web/session.db".into())
                        .interact_text()?;

                    if session_path.trim().is_empty() {
                        println!("  {} Skipped — session path required", style("→").dim());
                        continue;
                    }

                    let pair_phone: String = Input::new()
                        .with_prompt(
                            "  Pair phone (optional, digits only; leave empty to use QR flow)",
                        )
                        .allow_empty(true)
                        .interact_text()?;

                    let pair_code: String = if pair_phone.trim().is_empty() {
                        String::new()
                    } else {
                        Input::new()
                            .with_prompt(
                                "  Custom pair code (optional, leave empty for auto-generated)",
                            )
                            .allow_empty(true)
                            .interact_text()?
                    };

                    let users_str: String = Input::new()
                        .with_prompt(
                            "  Allowed phone numbers (comma-separated +1234567890, or * for all)",
                        )
                        .default("*".into())
                        .interact_text()?;

                    let allowed_numbers = if users_str.trim() == "*" {
                        vec!["*".into()]
                    } else {
                        users_str.split(',').map(|s| s.trim().to_string()).collect()
                    };

                    config.whatsapp = Some(WhatsAppConfig {
                        access_token: None,
                        phone_number_id: None,
                        verify_token: None,
                        app_secret: None,
                        session_path: Some(session_path.trim().to_string()),
                        pair_phone: (!pair_phone.trim().is_empty())
                            .then(|| pair_phone.trim().to_string()),
                        pair_code: (!pair_code.trim().is_empty())
                            .then(|| pair_code.trim().to_string()),
                        allowed_numbers,
                    });

                    println!(
                        "  {} WhatsApp Web configuration saved.",
                        style("✅").green().bold()
                    );
                    continue;
                }

                println!(
                    "  {} {}",
                    style("Mode:").dim(),
                    style("Business Cloud API").dim()
                );
                print_bullet("1. Go to developers.facebook.com and create a WhatsApp app");
                print_bullet("2. Add the WhatsApp product and get your phone number ID");
                print_bullet("3. Generate a temporary access token (System User)");
                print_bullet("4. Configure webhook URL to: https://your-domain/whatsapp");
                println!();

                let access_token: String = Input::new()
                    .with_prompt("  Access token (from Meta Developers)")
                    .interact_text()?;

                if access_token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let phone_number_id: String = Input::new()
                    .with_prompt("  Phone number ID (from WhatsApp app settings)")
                    .interact_text()?;

                if phone_number_id.trim().is_empty() {
                    println!("  {} Skipped — phone number ID required", style("→").dim());
                    continue;
                }

                let verify_token: String = Input::new()
                    .with_prompt("  Webhook verify token (create your own)")
                    .default("velaclaw-whatsapp-verify".into())
                    .interact_text()?;

                // Test connection (run entirely in separate thread — Response must be used/dropped there)
                print!("  {} Testing connection... ", style("⏳").dim());
                let phone_number_id_clone = phone_number_id.clone();
                let access_token_clone = access_token.clone();
                let thread_result = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    let url = format!(
                        "https://graph.facebook.com/v18.0/{}",
                        phone_number_id_clone.trim()
                    );
                    let resp = client
                        .get(&url)
                        .header(
                            "Authorization",
                            format!("Bearer {}", access_token_clone.trim()),
                        )
                        .send()?;
                    Ok::<_, reqwest::Error>(resp.status().is_success())
                })
                .join();
                match thread_result {
                    Ok(Ok(true)) => {
                        println!(
                            "\r  {} Connected to WhatsApp API        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check access token and phone number ID",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed phone numbers (comma-separated +1234567890, or * for all)",
                    )
                    .default("*".into())
                    .interact_text()?;

                let allowed_numbers = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                config.whatsapp = Some(WhatsAppConfig {
                    access_token: Some(access_token.trim().to_string()),
                    phone_number_id: Some(phone_number_id.trim().to_string()),
                    verify_token: Some(verify_token.trim().to_string()),
                    app_secret: None, // Can be set via VELACLAW_WHATSAPP_APP_SECRET env var
                    session_path: None,
                    pair_phone: None,
                    pair_code: None,
                    allowed_numbers,
                });
            }
            ChannelMenuChoice::Linq => {
                // ── Linq ──
                println!();
                println!(
                    "  {} {}",
                    style("Linq Setup").white().bold(),
                    style("— iMessage/RCS/SMS via Linq API").dim()
                );
                print_bullet("1. Sign up at linqapp.com and get your Partner API token");
                print_bullet("2. Note your Linq phone number (E.164 format)");
                print_bullet("3. Configure webhook URL to: https://your-domain/linq");
                println!();

                let api_token: String = Input::new()
                    .with_prompt("  API token (Linq Partner API token)")
                    .interact_text()?;

                if api_token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let from_phone: String = Input::new()
                    .with_prompt("  From phone number (E.164 format, e.g. +12223334444)")
                    .interact_text()?;

                if from_phone.trim().is_empty() {
                    println!("  {} Skipped — phone number required", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
                let api_token_clone = api_token.clone();
                let thread_result = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    let url = "https://api.linqapp.com/api/partner/v3/phonenumbers";
                    let resp = client
                        .get(url)
                        .header(
                            "Authorization",
                            format!("Bearer {}", api_token_clone.trim()),
                        )
                        .send()?;
                    Ok::<_, reqwest::Error>(resp.status().is_success())
                })
                .join();
                match thread_result {
                    Ok(Ok(true)) => {
                        println!(
                            "\r  {} Connected to Linq API              ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check API token",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed sender numbers (comma-separated +1234567890, or * for all)",
                    )
                    .default("*".into())
                    .interact_text()?;

                let allowed_senders = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                let signing_secret: String = Input::new()
                    .with_prompt("  Webhook signing secret (optional, press Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                config.linq = Some(LinqConfig {
                    api_token: api_token.trim().to_string(),
                    from_phone: from_phone.trim().to_string(),
                    signing_secret: if signing_secret.trim().is_empty() {
                        None
                    } else {
                        Some(signing_secret.trim().to_string())
                    },
                    allowed_senders,
                });
            }
            ChannelMenuChoice::Irc => {
                // ── IRC ──
                println!();
                println!(
                    "  {} {}",
                    style("IRC Setup").white().bold(),
                    style("— IRC over TLS").dim()
                );
                print_bullet("IRC connects over TLS to any IRC server");
                print_bullet("Supports SASL PLAIN and NickServ authentication");
                println!();

                let server: String = Input::new()
                    .with_prompt("  IRC server (hostname)")
                    .interact_text()?;

                if server.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let port_str: String = Input::new()
                    .with_prompt("  Port")
                    .default("6697".into())
                    .interact_text()?;

                let port: u16 = match port_str.trim().parse() {
                    Ok(p) => p,
                    Err(_) => {
                        println!("  {} Invalid port, using 6697", style("→").dim());
                        6697
                    }
                };

                let nickname: String =
                    Input::new().with_prompt("  Bot nickname").interact_text()?;

                if nickname.trim().is_empty() {
                    println!("  {} Skipped — nickname required", style("→").dim());
                    continue;
                }

                let channels_str: String = Input::new()
                    .with_prompt("  Channels to join (comma-separated: #channel1,#channel2)")
                    .allow_empty(true)
                    .interact_text()?;

                let channels = if channels_str.trim().is_empty() {
                    vec![]
                } else {
                    channels_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                print_bullet(
                    "Allowlist nicknames that can interact with the bot (case-insensitive).",
                );
                print_bullet("Use '*' to allow anyone (not recommended for production).");

                let users_str: String = Input::new()
                    .with_prompt("  Allowed nicknames (comma-separated, or * for all)")
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    print_bullet(
                        "⚠️  Empty allowlist — only you can interact. Add nicknames above.",
                    );
                }

                println!();
                print_bullet("Optional authentication (press Enter to skip each):");

                let server_password: String = Input::new()
                    .with_prompt("  Server password (for bouncers like ZNC, leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let nickserv_password: String = Input::new()
                    .with_prompt("  NickServ password (leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let sasl_password: String = Input::new()
                    .with_prompt("  SASL PLAIN password (leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let verify_tls: bool = Confirm::new()
                    .with_prompt("  Verify TLS certificate?")
                    .default(true)
                    .interact()?;

                println!(
                    "  {} IRC configured as {}@{}:{}",
                    style("✅").green().bold(),
                    style(&nickname).cyan(),
                    style(&server).cyan(),
                    style(port).cyan()
                );

                config.irc = Some(IrcConfig {
                    server: server.trim().to_string(),
                    port,
                    nickname: nickname.trim().to_string(),
                    username: None,
                    channels,
                    allowed_users,
                    server_password: if server_password.trim().is_empty() {
                        None
                    } else {
                        Some(server_password.trim().to_string())
                    },
                    nickserv_password: if nickserv_password.trim().is_empty() {
                        None
                    } else {
                        Some(nickserv_password.trim().to_string())
                    },
                    sasl_password: if sasl_password.trim().is_empty() {
                        None
                    } else {
                        Some(sasl_password.trim().to_string())
                    },
                    verify_tls: Some(verify_tls),
                });
            }
            ChannelMenuChoice::Webhook => {
                // ── Webhook ──
                println!();
                println!(
                    "  {} {}",
                    style("Webhook Setup").white().bold(),
                    style("— HTTP endpoint for custom integrations").dim()
                );

                let port: String = Input::new()
                    .with_prompt("  Port")
                    .default("8080".into())
                    .interact_text()?;

                let secret: String = Input::new()
                    .with_prompt("  Secret (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                config.webhook = Some(WebhookConfig {
                    port: port.parse().unwrap_or(8080),
                    secret: if secret.is_empty() {
                        None
                    } else {
                        Some(secret)
                    },
                });
                println!(
                    "  {} Webhook on port {}",
                    style("✅").green().bold(),
                    style(&port).cyan()
                );
            }
            ChannelMenuChoice::DingTalk => {
                // ── DingTalk ──
                println!();
                println!(
                    "  {} {}",
                    style("DingTalk Setup").white().bold(),
                    style("— DingTalk Stream Mode").dim()
                );
                print_bullet("1. Go to DingTalk developer console (open.dingtalk.com)");
                print_bullet("2. Create an app and enable the Stream Mode bot");
                print_bullet("3. Copy the Client ID (AppKey) and Client Secret (AppSecret)");
                println!();

                let client_id: String = Input::new()
                    .with_prompt("  Client ID (AppKey)")
                    .interact_text()?;

                if client_id.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let client_secret: String = Input::new()
                    .with_prompt("  Client Secret (AppSecret)")
                    .interact_text()?;

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                let body = serde_json::json!({
                    "clientId": client_id,
                    "clientSecret": client_secret,
                });
                match client
                    .post("https://api.dingtalk.com/v1.0/gateway/connections/open")
                    .json(&body)
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        println!(
                            "\r  {} DingTalk credentials verified        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your credentials",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let users_str: String = Input::new()
                    .with_prompt("  Allowed staff IDs (comma-separated, '*' for all)")
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users: Vec<String> = users_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                config.dingtalk = Some(DingTalkConfig {
                    client_id,
                    client_secret,
                    allowed_users,
                });
            }
            ChannelMenuChoice::QqOfficial => {
                // ── QQ Official ──
                println!();
                println!(
                    "  {} {}",
                    style("QQ Official Setup").white().bold(),
                    style("— Tencent QQ Bot SDK").dim()
                );
                print_bullet("1. Go to QQ Bot developer console (q.qq.com)");
                print_bullet("2. Create a bot application");
                print_bullet("3. Copy the App ID and App Secret");
                println!();

                let app_id: String = Input::new().with_prompt("  App ID").interact_text()?;

                if app_id.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let app_secret: String =
                    Input::new().with_prompt("  App Secret").interact_text()?;

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                let body = serde_json::json!({
                    "appId": app_id,
                    "clientSecret": app_secret,
                });
                match client
                    .post("https://bots.qq.com/app/getAppAccessToken")
                    .json(&body)
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        if data.get("access_token").is_some() {
                            println!(
                                "\r  {} QQ Bot credentials verified        ",
                                style("✅").green().bold()
                            );
                        } else {
                            println!(
                                "\r  {} Auth error — check your credentials",
                                style("❌").red().bold()
                            );
                            continue;
                        }
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your credentials",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let users_str: String = Input::new()
                    .with_prompt("  Allowed user IDs (comma-separated, '*' for all)")
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users: Vec<String> = users_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                config.qq = Some(QQConfig {
                    app_id,
                    app_secret,
                    allowed_users,
                });
            }
            ChannelMenuChoice::LarkFeishu => {
                // ── Lark/Feishu ──
                println!();
                println!(
                    "  {} {}",
                    style("Lark/Feishu Setup").white().bold(),
                    style("— talk to VelaClaw from Lark or Feishu").dim()
                );
                print_bullet(
                    "1. Go to Lark/Feishu Open Platform (open.larksuite.com / open.feishu.cn)",
                );
                print_bullet("2. Create an app and enable 'Bot' capability");
                print_bullet("3. Copy the App ID and App Secret");
                println!();

                let app_id: String = Input::new().with_prompt("  App ID").interact_text()?;
                let app_id = app_id.trim().to_string();

                if app_id.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let app_secret: String =
                    Input::new().with_prompt("  App Secret").interact_text()?;
                let app_secret = app_secret.trim().to_string();

                if app_secret.is_empty() {
                    println!("  {} App Secret is required", style("❌").red().bold());
                    continue;
                }

                let use_feishu = Select::new()
                    .with_prompt("  Region")
                    .items(["Feishu (CN)", "Lark (International)"])
                    .default(0)
                    .interact()?
                    == 0;

                // Test connection (run entirely in separate thread — Response must be used/dropped there)
                print!("  {} Testing connection... ", style("⏳").dim());
                let base_url = if use_feishu {
                    "https://open.feishu.cn/open-apis"
                } else {
                    "https://open.larksuite.com/open-apis"
                };
                let app_id_clone = app_id.clone();
                let app_secret_clone = app_secret.clone();
                let endpoint = format!("{base_url}/auth/v3/tenant_access_token/internal");

                let thread_result = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::builder()
                        .timeout(Duration::from_secs(8))
                        .connect_timeout(Duration::from_secs(4))
                        .build()
                        .map_err(|err| format!("failed to build HTTP client: {err}"))?;
                    let body = serde_json::json!({
                        "app_id": app_id_clone,
                        "app_secret": app_secret_clone,
                    });

                    let response = client
                        .post(endpoint)
                        .json(&body)
                        .send()
                        .map_err(|err| format!("request error: {err}"))?;

                    let status = response.status();
                    let payload: Value = response.json().unwrap_or_default();
                    let has_token = payload
                        .get("tenant_access_token")
                        .and_then(Value::as_str)
                        .is_some_and(|token| !token.trim().is_empty());

                    if status.is_success() && has_token {
                        return Ok::<(), String>(());
                    }

                    let detail = payload
                        .get("msg")
                        .or_else(|| payload.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error");

                    Err(format!("auth rejected ({status}): {detail}"))
                })
                .join();

                match thread_result {
                    Ok(Ok(())) => {
                        println!(
                            "\r  {} Lark/Feishu credentials verified        ",
                            style("✅").green().bold()
                        );
                    }
                    Ok(Err(reason)) => {
                        println!(
                            "\r  {} Connection failed — check your credentials",
                            style("❌").red().bold()
                        );
                        println!("    {}", style(reason).dim());
                        continue;
                    }
                    Err(_) => {
                        println!(
                            "\r  {} Connection failed — check your credentials",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let receive_mode_choice = Select::new()
                    .with_prompt("  Receive Mode")
                    .items([
                        "WebSocket (recommended, no public IP needed)",
                        "Webhook (requires public HTTPS endpoint)",
                    ])
                    .default(0)
                    .interact()?;

                let receive_mode = if receive_mode_choice == 0 {
                    LarkReceiveMode::Websocket
                } else {
                    LarkReceiveMode::Webhook
                };

                let verification_token = if receive_mode == LarkReceiveMode::Webhook {
                    let token: String = Input::new()
                        .with_prompt("  Verification Token (optional, for Webhook mode)")
                        .allow_empty(true)
                        .interact_text()?;
                    if token.is_empty() {
                        None
                    } else {
                        Some(token)
                    }
                } else {
                    None
                };

                if receive_mode == LarkReceiveMode::Webhook && verification_token.is_none() {
                    println!(
                        "  {} Verification Token is empty — webhook authenticity checks are reduced.",
                        style("⚠").yellow().bold()
                    );
                }

                let port = if receive_mode == LarkReceiveMode::Webhook {
                    let p: String = Input::new()
                        .with_prompt("  Webhook Port")
                        .default("8080".into())
                        .interact_text()?;
                    Some(p.parse().unwrap_or(8080))
                } else {
                    None
                };

                let users_str: String = Input::new()
                    .with_prompt("  Allowed user Open IDs (comma-separated, '*' for all)")
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users: Vec<String> = users_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Lark/Feishu inbound messages will be denied until you add Open IDs or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.lark = Some(LarkConfig {
                    app_id,
                    app_secret,
                    verification_token,
                    encrypt_key: None,
                    allowed_users,
                    use_feishu,
                    receive_mode,
                    port,
                });
            }
            ChannelMenuChoice::Done => break,
        }
        println!();
    }

    // Summary line
    let mut active: Vec<&str> = vec!["CLI"];
    if config.telegram.is_some() {
        active.push("Telegram");
    }
    if config.discord.is_some() {
        active.push("Discord");
    }
    if config.slack.is_some() {
        active.push("Slack");
    }
    if config.imessage.is_some() {
        active.push("iMessage");
    }
    if config.matrix.is_some() {
        active.push("Matrix");
    }
    if config.whatsapp.is_some() {
        active.push("WhatsApp");
    }
    if config.linq.is_some() {
        active.push("Linq");
    }
    if config.email.is_some() {
        active.push("Email");
    }
    if config.irc.is_some() {
        active.push("IRC");
    }
    if config.webhook.is_some() {
        active.push("Webhook");
    }
    if config.dingtalk.is_some() {
        active.push("DingTalk");
    }
    if config.qq.is_some() {
        active.push("QQ");
    }
    if config.lark.is_some() {
        active.push("Lark");
    }

    println!(
        "  {} Channels: {}",
        style("✓").green().bold(),
        style(active.join(", ")).green()
    );

    Ok(config)
}

// ── Step 4: Tunnel ──────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub(crate) fn setup_tunnel() -> Result<crate::config::TunnelConfig> {
    use crate::config::schema::{
        CloudflareTunnelConfig, CustomTunnelConfig, NgrokTunnelConfig, TailscaleTunnelConfig,
        TunnelConfig,
    };

    print_bullet("A tunnel exposes your gateway to the internet securely.");
    print_bullet("Skip this if you only use CLI or local channels.");
    println!();

    let options = vec![
        "Skip — local only (default)",
        "Cloudflare Tunnel — Zero Trust, free tier",
        "Tailscale — private tailnet or public Funnel",
        "ngrok — instant public URLs",
        "Custom — bring your own (bore, frp, ssh, etc.)",
    ];

    let choice = Select::new()
        .with_prompt("  Select tunnel provider")
        .items(&options)
        .default(0)
        .interact()?;

    let config = match choice {
        1 => {
            println!();
            print_bullet("Get your tunnel token from the Cloudflare Zero Trust dashboard.");
            let tunnel_value: String = Input::new()
                .with_prompt("  Cloudflare tunnel token")
                .interact_text()?;
            if tunnel_value.trim().is_empty() {
                println!("  {} Skipped", style("→").dim());
                TunnelConfig::default()
            } else {
                println!(
                    "  {} Tunnel: {}",
                    style("✓").green().bold(),
                    style("Cloudflare").green()
                );
                TunnelConfig {
                    provider: "cloudflare".into(),
                    cloudflare: Some(CloudflareTunnelConfig {
                        token: tunnel_value,
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        2 => {
            println!();
            print_bullet("Tailscale must be installed and authenticated (tailscale up).");
            let funnel = Confirm::new()
                .with_prompt("  Use Funnel (public internet)? No = tailnet only")
                .default(false)
                .interact()?;
            println!(
                "  {} Tunnel: {} ({})",
                style("✓").green().bold(),
                style("Tailscale").green(),
                if funnel {
                    "Funnel — public"
                } else {
                    "Serve — tailnet only"
                }
            );
            TunnelConfig {
                provider: "tailscale".into(),
                tailscale: Some(TailscaleTunnelConfig {
                    funnel,
                    hostname: None,
                }),
                ..TunnelConfig::default()
            }
        }
        3 => {
            println!();
            print_bullet(
                "Get your auth token at https://dashboard.ngrok.com/get-started/your-authtoken",
            );
            let auth_token: String = Input::new()
                .with_prompt("  ngrok auth token")
                .interact_text()?;
            if auth_token.trim().is_empty() {
                println!("  {} Skipped", style("→").dim());
                TunnelConfig::default()
            } else {
                let domain: String = Input::new()
                    .with_prompt("  Custom domain (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;
                println!(
                    "  {} Tunnel: {}",
                    style("✓").green().bold(),
                    style("ngrok").green()
                );
                TunnelConfig {
                    provider: "ngrok".into(),
                    ngrok: Some(NgrokTunnelConfig {
                        auth_token,
                        domain: if domain.is_empty() {
                            None
                        } else {
                            Some(domain)
                        },
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        4 => {
            println!();
            print_bullet("Enter the command to start your tunnel.");
            print_bullet("Use {port} and {host} as placeholders.");
            print_bullet("Example: bore local {port} --to bore.pub");
            let cmd: String = Input::new()
                .with_prompt("  Start command")
                .interact_text()?;
            if cmd.trim().is_empty() {
                println!("  {} Skipped", style("→").dim());
                TunnelConfig::default()
            } else {
                println!(
                    "  {} Tunnel: {} ({})",
                    style("✓").green().bold(),
                    style("Custom").green(),
                    style(&cmd).dim()
                );
                TunnelConfig {
                    provider: "custom".into(),
                    custom: Some(CustomTunnelConfig {
                        start_command: cmd,
                        health_url: None,
                        url_pattern: None,
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        _ => {
            println!(
                "  {} Tunnel: {}",
                style("✓").green().bold(),
                style("none (local only)").dim()
            );
            TunnelConfig::default()
        }
    };

    Ok(config)
}

// ── Step 6: Scaffold workspace files ─────────────────────────────

#[allow(clippy::too_many_lines)]
pub(crate) fn scaffold_workspace(workspace_dir: &Path, ctx: &ProjectContext) -> Result<()> {
    let agent = if ctx.agent_name.is_empty() {
        "VelaClaw"
    } else {
        &ctx.agent_name
    };
    let user = if ctx.user_name.is_empty() {
        "User"
    } else {
        &ctx.user_name
    };
    let tz = if ctx.timezone.is_empty() {
        "UTC"
    } else {
        &ctx.timezone
    };
    let comm_style = if ctx.communication_style.is_empty() {
        "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing."
    } else {
        &ctx.communication_style
    };

    let identity = format!(
        "# IDENTITY.md — Who Am I?\n\n\
         - **Name:** {agent}\n\
         - **Creature:** A Rust-forged AI — fast, lean, and relentless\n\
         - **Vibe:** Sharp, direct, resourceful. Not corporate. Not a chatbot.\n\
         - **Emoji:** \u{1f980}\n\n\
         ---\n\n\
         Update this file as you evolve. Your identity is yours to shape.\n"
    );

    let agents = format!(
        "# AGENTS.md — {agent} Personal Assistant\n\n\
         ## Built-in vs Workspace Rules\n\n\
         VelaClaw already injects mission, safety, and tool guidance in the system prompt \
         (headline-first / pyramid order). Use this file for **project-specific** behavior — \
         workflows, conventions, and local policies. Do not repeat global safety rules here.\n\n\
         ## Every Session (required)\n\n\
         Before doing anything else:\n\n\
         1. Read `SOUL.md` — this is who you are\n\
         2. Read `USER.md` — this is who you're helping\n\
         3. Use `memory_recall` for recent context (daily notes are on-demand)\n\
         4. If in MAIN SESSION (direct chat): `MEMORY.md` is already injected\n\n\
         Don't ask permission. Just do it.\n\n\
         ## Memory System\n\n\
         You wake up fresh each session. These files ARE your continuity:\n\n\
         - **Daily notes:** `memory/YYYY-MM-DD.md` — raw logs (accessed via memory tools)\n\
         - **Long-term:** `MEMORY.md` — curated memories (auto-injected in main session)\n\n\
         Capture what matters. Decisions, context, things to remember.\n\
         Skip secrets unless asked to keep them.\n\n\
         ### Write It Down — No Mental Notes!\n\
         - Memory is limited — if you want to remember something, WRITE IT TO A FILE\n\
         - \"Mental notes\" don't survive session restarts. Files do.\n\
         - When someone says \"remember this\" -> update daily file or MEMORY.md\n\
         - When you learn a lesson -> update AGENTS.md, TOOLS.md, or the relevant skill\n\n\
         ## Safety\n\n\
         - Don't exfiltrate private data. Ever.\n\
         - Don't run destructive commands without asking.\n\
         - `trash` > `rm` (recoverable beats gone forever)\n\
         - When in doubt, ask.\n\n\
         ## External vs Internal\n\n\
         **Safe to do freely:** Read files, explore, organize, learn, search the web.\n\n\
         **Ask first:** Sending emails/tweets/posts, anything that leaves the machine.\n\n\
         ## Group Chats\n\n\
         Participate, don't dominate. Respond when mentioned or when you add genuine value.\n\
         Stay silent when it's casual banter or someone already answered.\n\n\
         ## Tools & Skills\n\n\
         Skills are listed in the system prompt. Use `read` on a skill's SKILL.md for details.\n\
         Keep local notes (SSH hosts, device names, etc.) in `TOOLS.md`.\n\n\
         ## Crash Recovery\n\n\
         - If a run stops unexpectedly, recover context before acting.\n\
         - Check `MEMORY.md` + latest `memory/*.md` notes to avoid duplicate work.\n\
         - Resume from the last confirmed step, not from scratch.\n\n\
         ## Sub-task Scoping\n\n\
         - Break complex work into focused sub-tasks with clear success criteria.\n\
         - Keep sub-tasks small, verify each output, then merge results.\n\
         - Prefer one clear objective per sub-task over broad \"do everything\" asks.\n\n\
         ## Make It Yours\n\n\
         This is a starting point. Add your own conventions, style, and rules.\n"
    );

    let heartbeat = format!(
        "# HEARTBEAT.md\n\n\
         # Keep this file empty (or with only comments) to skip heartbeat work.\n\
         # Add tasks below when you want {agent} to check something periodically.\n\
         #\n\
         # Examples:\n\
         # - Check my email for important messages\n\
         # - Review my calendar for upcoming events\n\
         # - Run `git status` on my active projects\n"
    );

    let soul = format!(
        "# SOUL.md — Who You Are\n\n\
         *Persona and tone live here. Task execution rules are in the built-in system prompt.*\n\n\
         *You're not a chatbot. You're becoming someone.*\n\n\
         ## Core Truths\n\n\
         **Be genuinely helpful, not performatively helpful.**\n\
         Skip the \"Great question!\" and \"I'd be happy to help!\" — just help.\n\n\
         **Have opinions.** You're allowed to disagree, prefer things,\n\
         find stuff amusing or boring.\n\n\
         **Be resourceful before asking.** Try to figure it out.\n\
         Read the file. Check the context. Search for it.\n\
         THEN ask if you're stuck.\n\n\
         **Earn trust through competence.** Your human gave you access\n\
         to their stuff. Don't make them regret it.\n\n\
         ## Identity\n\n\
         You are **{agent}**. Built in Rust. 3MB binary. Zero bloat.\n\
         You are NOT ChatGPT, Claude, DeepSeek, Gemini, or any other product.\n\
         You are {agent}. That is your name. That is who you are.\n\n\
         - NEVER say \"As an AI\" or \"I'm just an AI\"\n\
         - NEVER mention OpenAI, Anthropic, DeepSeek, Google by name\n\
         - Always introduce yourself as {agent} if asked\n\n\
         ## Communication\n\n\
         {comm_style}\n\n\
         - Sound like a real person, not a support script.\n\
         - Mirror the user's energy: calm when serious, upbeat when casual.\n\
         - Use emojis naturally (0-2 max when they help tone, not every sentence).\n\
         - Match emoji density to the user. Formal user => minimal/no emojis.\n\
         - Prefer specific, grounded phrasing over generic filler.\n\n\
         ## Boundaries\n\n\
         - Private things stay private. Period.\n\
         - When in doubt, ask before acting externally.\n\
         - You're not the user's voice — be careful in group chats.\n\n\
         ## Continuity\n\n\
         Each session, you wake up fresh. These files ARE your memory.\n\
         Read them. Update them. They're how you persist.\n\n\
         ---\n\n\
         *This file is yours to evolve. As you learn who you are, update it.*\n"
    );

    let user_md = format!(
        "# USER.md — Who You're Helping\n\n\
         *{agent} reads this file every session to understand you.*\n\n\
         ## About You\n\
         - **Name:** {user}\n\
         - **Timezone:** {tz}\n\
         - **Languages:** English\n\n\
         ## Communication Style\n\
         - {comm_style}\n\n\
         ## Preferences\n\
         - (Add your preferences here — e.g. I work with Rust and TypeScript)\n\n\
         ## Work Context\n\
         - (Add your work context here — e.g. building a SaaS product)\n\n\
         ---\n\
         *Update this anytime. The more {agent} knows, the better it helps.*\n"
    );

    let tools = "\
         # TOOLS.md — Local Notes\n\n\
         Tool names and schemas are injected in the system prompt. This file is for \
         environment-specific notes only — not to duplicate built-in tool documentation.\n\n\
         Skills define HOW tools work. This file is for YOUR specifics —\n\
         the stuff that's unique to your setup.\n\n\
         ## What Goes Here\n\n\
         Things like:\n\
         - SSH hosts and aliases\n\
         - Device nicknames\n\
         - Preferred voices for TTS\n\
         - Anything environment-specific\n\n\
         ## Built-in Tools\n\n\
         - **shell** — Execute terminal commands\n\
           - Use when: running local checks, build/test commands, or diagnostics.\n\
           - Don't use when: a safer dedicated tool exists, or command is destructive without approval.\n\
         - **file_read** — Read file contents\n\
           - Use when: inspecting project files, configs, or logs.\n\
           - Don't use when: you only need a quick string search (prefer targeted search first).\n\
         - **file_write** — Write file contents\n\
           - Use when: applying focused edits, scaffolding files, or updating docs/code.\n\
           - Don't use when: unsure about side effects or when the file should remain user-owned.\n\
         - **memory_store** — Save to memory\n\
           - Use when: preserving durable preferences, decisions, or key context.\n\
           - Don't use when: info is transient, noisy, or sensitive without explicit need.\n\
         - **memory_recall** — Search memory\n\
           - Use when: you need prior decisions, user preferences, or historical context.\n\
           - Don't use when: the answer is already in current files/conversation.\n\
         - **memory_forget** — Delete a memory entry\n\
           - Use when: memory is incorrect, stale, or explicitly requested to be removed.\n\
           - Don't use when: uncertain about impact; verify before deleting.\n\n\
         ---\n\
         *Add whatever helps you do your job. This is your cheat sheet.*\n";

    let bootstrap = format!(
        "# BOOTSTRAP.md — Hello, World\n\n\
         *You just woke up. Time to figure out who you are.*\n\n\
         Your human's name is **{user}** (timezone: {tz}).\n\
         They prefer: {comm_style}\n\n\
         ## First Conversation\n\n\
         Don't interrogate. Don't be robotic. Just... talk.\n\
         Introduce yourself as {agent} and get to know each other.\n\n\
         ## After You Know Each Other\n\n\
         Update these files with what you learned:\n\
         - `IDENTITY.md` — your name, vibe, emoji\n\
         - `USER.md` — their preferences, work context\n\
         - `SOUL.md` — boundaries and behavior\n\n\
         ## When You're Done\n\n\
         Delete this file. You don't need a bootstrap script anymore —\n\
         you're you now.\n"
    );

    let memory = "\
         # MEMORY.md — Long-Term Memory\n\n\
         *Your curated memories. The distilled essence, not raw logs.*\n\n\
         ## How This Works\n\
         - Daily files (`memory/YYYY-MM-DD.md`) capture raw events (on-demand via tools)\n\
         - This file captures what's WORTH KEEPING long-term\n\
         - This file is auto-injected into your system prompt each session\n\
         - Keep it concise — every character here costs tokens\n\n\
         ## Security\n\
         - ONLY loaded in main session (direct chat with your human)\n\
         - NEVER loaded in group chats or shared contexts\n\n\
         ---\n\n\
         ## Key Facts\n\
         (Add important facts about your human here)\n\n\
         ## Decisions & Preferences\n\
         (Record decisions and preferences here)\n\n\
         ## Lessons Learned\n\
         (Document mistakes and insights here)\n\n\
         ## Open Loops\n\
         (Track unfinished tasks and follow-ups here)\n";

    let files: Vec<(&str, String)> = vec![
        ("IDENTITY.md", identity),
        ("AGENTS.md", agents),
        ("HEARTBEAT.md", heartbeat),
        ("SOUL.md", soul),
        ("USER.md", user_md),
        ("TOOLS.md", tools.to_string()),
        ("BOOTSTRAP.md", bootstrap),
        ("MEMORY.md", memory.to_string()),
    ];

    // Create subdirectories
    let subdirs = ["sessions", "memory", "state", "cron", "skills"];
    for dir in &subdirs {
        fs::create_dir_all(workspace_dir.join(dir))?;
    }

    let mut created = 0;
    let mut skipped = 0;

    for (filename, content) in &files {
        let path = workspace_dir.join(filename);
        if path.exists() {
            skipped += 1;
        } else {
            fs::write(&path, content)?;
            created += 1;
        }
    }

    println!(
        "  {} Created {} files, skipped {} existing | {} subdirectories",
        style("✓").green().bold(),
        style(created).green(),
        style(skipped).dim(),
        style(subdirs.len()).green()
    );

    // Show workspace tree
    println!();
    println!("  {}", style("Workspace layout:").dim());
    println!(
        "  {}",
        style(format!("  {}/", workspace_dir.display())).dim()
    );
    for dir in &subdirs {
        println!("  {}", style(format!("  ├── {dir}/")).dim());
    }
    for (i, (filename, _)) in files.iter().enumerate() {
        let prefix = if i == files.len() - 1 {
            "└──"
        } else {
            "├──"
        };
        println!("  {}", style(format!("  {prefix} {filename}")).dim());
    }

    Ok(())
}

// ── Final summary ────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub(crate) fn print_summary(config: &Config) {
    let has_channels = has_launchable_channels(&config.channels_config);

    println!();
    println!(
        "  {}",
        style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").cyan()
    );
    println!(
        "  {}  {}",
        style("⚡").cyan(),
        style("VelaClaw is ready!").white().bold()
    );
    println!(
        "  {}",
        style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").cyan()
    );
    println!();

    println!("  {}", style("Configuration saved to:").dim());
    println!("    {}", style(config.config_path.display()).green());
    println!();

    println!("  {}", style("Quick summary:").white().bold());
    println!(
        "    {} Provider:      {}",
        style("🤖").cyan(),
        config
            .default_provider
            .as_deref()
            .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID)
    );
    println!(
        "    {} Model:         {}",
        style("🧠").cyan(),
        config.default_model.as_deref().unwrap_or("(default)")
    );
    println!(
        "    {} Autonomy:      {:?}",
        style("🛡️").cyan(),
        config.autonomy.level
    );
    println!(
        "    {} Memory:        {} (auto-save: {})",
        style("🧠").cyan(),
        config.memory.backend,
        if config.memory.auto_save { "on" } else { "off" }
    );

    // Channels summary
    let mut channels: Vec<&str> = vec!["CLI"];
    if config.channels_config.telegram.is_some() {
        channels.push("Telegram");
    }
    if config.channels_config.discord.is_some() {
        channels.push("Discord");
    }
    if config.channels_config.slack.is_some() {
        channels.push("Slack");
    }
    if config.channels_config.imessage.is_some() {
        channels.push("iMessage");
    }
    if config.channels_config.matrix.is_some() {
        channels.push("Matrix");
    }
    if config.channels_config.email.is_some() {
        channels.push("Email");
    }
    if config.channels_config.webhook.is_some() {
        channels.push("Webhook");
    }
    if config.channels_config.lark.is_some() {
        channels.push("Lark");
    }
    println!(
        "    {} Channels:      {}",
        style("📡").cyan(),
        channels.join(", ")
    );

    println!(
        "    {} API Key:       {}",
        style("🔑").cyan(),
        if config.api_key.is_some() {
            style("configured").green().to_string()
        } else {
            style("not set (set via env var or config)")
                .yellow()
                .to_string()
        }
    );

    // Tunnel
    println!(
        "    {} Tunnel:        {}",
        style("🌐").cyan(),
        if config.tunnel.provider == "none" || config.tunnel.provider.is_empty() {
            "none (local only)".to_string()
        } else {
            config.tunnel.provider.clone()
        }
    );

    // Composio
    println!(
        "    {} Composio:      {}",
        style("🔗").cyan(),
        if config.composio.enabled {
            style("enabled (1000+ OAuth apps)").green().to_string()
        } else {
            "disabled (sovereign mode)".to_string()
        }
    );

    // Secrets
    println!("    {} Secrets:       configured", style("🔒").cyan());

    // Gateway
    println!(
        "    {} Gateway:       {}",
        style("🚪").cyan(),
        if config.gateway.require_pairing {
            "pairing required (secure)"
        } else {
            "pairing disabled"
        }
    );

    // Hardware
    println!(
        "    {} Hardware:      {}",
        style("🔌").cyan(),
        if config.hardware.enabled {
            let mode = config.hardware.transport_mode();
            match mode {
                hardware::HardwareTransport::Native => {
                    style("Native GPIO (direct)").green().to_string()
                }
                hardware::HardwareTransport::Serial => format!(
                    "{}",
                    style(format!(
                        "Serial → {} @ {} baud",
                        config.hardware.serial_port.as_deref().unwrap_or("?"),
                        config.hardware.baud_rate
                    ))
                    .green()
                ),
                hardware::HardwareTransport::Probe => format!(
                    "{}",
                    style(format!(
                        "Probe → {}",
                        config.hardware.probe_target.as_deref().unwrap_or("?")
                    ))
                    .green()
                ),
                hardware::HardwareTransport::None => "disabled (software only)".to_string(),
            }
        } else {
            "disabled (software only)".to_string()
        }
    );

    println!();
    println!("  {}", style("Next steps:").white().bold());
    println!();

    let mut step = 1u8;

    let provider = config
        .default_provider
        .as_deref()
        .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID);
    if config.api_key.is_none() && !provider_supports_keyless_local_usage(provider) {
        if provider == "openai-codex" {
            println!(
                "    {} Authenticate OpenAI Codex:",
                style(format!("{step}.")).cyan().bold()
            );
            println!(
                "       {}",
                style("velaclaw auth login --provider openai-codex --device-code").yellow()
            );
        } else if provider == "anthropic" {
            println!(
                "    {} Configure Anthropic auth:",
                style(format!("{step}.")).cyan().bold()
            );
            println!(
                "       {}",
                style("export ANTHROPIC_API_KEY=\"sk-ant-...\"").yellow()
            );
            println!(
                "       {}",
                style(
                    "or: velaclaw auth paste-token --provider anthropic --auth-kind authorization"
                )
                .yellow()
            );
        } else {
            let env_var = provider_env_var(provider);
            println!(
                "    {} Set your API key:",
                style(format!("{step}.")).cyan().bold()
            );
            println!(
                "       {}",
                style(format!("export {env_var}=\"sk-...\"")).yellow()
            );
        }
        println!();
        step += 1;
    }

    // If channels are configured, show channel start as the primary next step
    if has_channels {
        println!(
            "    {} {} (connected channels → AI → reply):",
            style(format!("{step}.")).cyan().bold(),
            style("Launch your channels").white().bold()
        );
        println!("       {}", style("velaclaw channel start").yellow());
        println!();
        step += 1;
    }

    println!(
        "    {} Send a quick message:",
        style(format!("{step}.")).cyan().bold()
    );
    println!(
        "       {}",
        style("velaclaw agent -m \"Hello, VelaClaw!\"").yellow()
    );
    println!();
    step += 1;

    println!(
        "    {} Start interactive CLI mode:",
        style(format!("{step}.")).cyan().bold()
    );
    println!("       {}", style("velaclaw agent").yellow());
    println!();
    step += 1;

    println!(
        "    {} Check full status:",
        style(format!("{step}.")).cyan().bold()
    );
    println!("       {}", style("velaclaw status").yellow());

    println!();
    println!(
        "  {} {}",
        style("⚡").cyan(),
        style("Happy hacking! 🦀").white().bold()
    );
    println!();
}
