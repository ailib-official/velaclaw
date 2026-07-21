//! VelaClaw 主入口程序，负责命令行解析与各子系统初始化。
#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::assigning_clones,
    clippy::bool_to_int_with_if,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::cast_possible_wrap,
    clippy::doc_markdown,
    clippy::field_reassign_with_default,
    clippy::float_cmp,
    clippy::implicit_clone,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::manual_let_else,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::needless_pass_by_value,
    clippy::needless_raw_string_hashes,
    clippy::redundant_closure_for_method_calls,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unused_self,
    clippy::cast_precision_loss,
    clippy::unnecessary_cast,
    clippy::unnecessary_lazy_evaluations,
    clippy::unnecessary_literal_bound,
    clippy::unnecessary_map_or,
    clippy::unnecessary_wraps,
    dead_code
)]

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use dialoguer::{Input, Password};
use serde::{Deserialize, Serialize};
use std::io::Write;
use tracing::warn;
use tracing_subscriber::{fmt, EnvFilter};

mod cli_dispatch;

fn parse_temperature(s: &str) -> std::result::Result<f64, String> {
    let t: f64 = s.parse().map_err(|e| format!("{e}"))?;
    if !(0.0..=2.0).contains(&t) {
        return Err("temperature must be between 0.0 and 2.0".to_string());
    }
    Ok(t)
}

// Binary-only modules (not part of the `velaclaw` library crate).
mod deploy;
mod skillforge;

// Thin binary: shared implementation lives in the library crate (`src/lib.rs`).
// Do not re-declare `mod agent;` etc. here — that compiles every source file twice
// and breaks whenever a new lib module (e.g. `execution`) is added without mirroring main.
use velaclaw::{
    auth, security, ChannelCommands, Config, CronCommands, HardwareCommands, IntegrationCommands,
    MemoryCommands, MigrateCommands, PeripheralCommands, ServiceCommands, SkillCommands,
};

/// `VelaClaw` - Protocol-driven autonomous AI agent runtime.
#[derive(Parser, Debug)]
#[command(name = "velaclaw")]
#[command(author = "Luqiang Wang")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Protocol-driven autonomous AI agent runtime with intelligent model selection.", long_about = None)]
struct Cli {
    #[arg(long, global = true)]
    config_dir: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CompletionShell {
    #[value(name = "bash")]
    Bash,
    #[value(name = "fish")]
    Fish,
    #[value(name = "zsh")]
    Zsh,
    #[value(name = "powershell")]
    PowerShell,
    #[value(name = "elvish")]
    Elvish,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize your workspace and configuration
    Onboard {
        /// Run the full interactive wizard (default is quick setup)
        #[arg(long)]
        interactive: bool,

        /// Overwrite existing config without confirmation
        #[arg(long)]
        force: bool,

        /// Reconfigure channels only (fast repair flow)
        #[arg(long)]
        channels_only: bool,

        /// API key (used in quick mode, ignored with --interactive)
        #[arg(long)]
        api_key: Option<String>,

        /// Provider/model id (used in quick mode, default: openai/gpt-5.2)
        #[arg(long)]
        provider: Option<String>,
        /// Model ID override (used in quick mode)
        #[arg(long)]
        model: Option<String>,
        /// Memory backend (sqlite, lucid, markdown, none) - used in quick mode, default: sqlite
        #[arg(long)]
        memory: Option<String>,
    },

    /// Start the AI agent loop
    #[command(long_about = "\
Start the AI agent loop.

Launches an interactive chat session with the configured AI provider. \
Use --message for single-shot queries without entering interactive mode.

Examples:
  velaclaw agent                              # interactive session
  velaclaw agent -m \"Summarize today's logs\"  # single message
  velaclaw agent -p anthropic --model claude-sonnet-4-20250514
  velaclaw agent --peripheral nucleo-f401re:/dev/ttyACM0")]
    Agent {
        /// Single message mode (don't enter interactive mode)
        #[arg(short, long)]
        message: Option<String>,

        /// Provider/model id to use (for example openai/gpt-5.2, or openai-codex)
        #[arg(short, long)]
        provider: Option<String>,

        /// Model to use
        #[arg(long)]
        model: Option<String>,

        /// Temperature (0.0 - 2.0)
        #[arg(short, long, default_value = "0.7", value_parser = parse_temperature)]
        temperature: f64,

        /// Attach a peripheral (board:path, e.g. nucleo-f401re:/dev/ttyACM0)
        #[arg(long)]
        peripheral: Vec<String>,

        /// Disable ANSI/Markdown terminal rendering (plain output)
        #[arg(long, default_value_t = false)]
        no_color: bool,

        /// Disable long-output folding in interactive REPL
        #[arg(long, default_value_t = false)]
        no_fold: bool,
    },

    /// Start the gateway server (webhooks, websockets)
    #[command(long_about = "\
Start the gateway server (webhooks, websockets).

Runs the HTTP/WebSocket gateway that accepts incoming webhook events \
and WebSocket connections. Bind address defaults to the values in \
your config file (gateway.host / gateway.port).

Examples:
  velaclaw gateway                  # use config defaults
  velaclaw gateway -p 8080          # listen on port 8080
  velaclaw gateway --host 0.0.0.0   # bind to all interfaces
  velaclaw gateway -p 0             # random available port")]
    Gateway {
        /// Port to listen on (use 0 for random available port); defaults to config gateway.port
        #[arg(short, long)]
        port: Option<u16>,

        /// Host to bind to; defaults to config gateway.host
        #[arg(long)]
        host: Option<String>,
    },

    /// Start long-running autonomous runtime (gateway + channels + heartbeat + scheduler)
    #[command(long_about = "\
Start the long-running autonomous daemon.

Launches the full VelaClaw runtime: gateway server, all configured \
channels (Telegram, Discord, Slack, etc.), heartbeat monitor, and \
the cron scheduler. This is the recommended way to run VelaClaw in \
production or as an always-on assistant.

Use 'velaclaw service install' to register the daemon as an OS \
service (systemd/launchd) for auto-start on boot.

Examples:
  velaclaw daemon                   # use config defaults
  velaclaw daemon -p 9090           # gateway on port 9090
  velaclaw daemon --host 127.0.0.1  # localhost only")]
    Daemon {
        /// Port to listen on (use 0 for random available port); defaults to config gateway.port
        #[arg(short, long)]
        port: Option<u16>,

        /// Host to bind to; defaults to config gateway.host
        #[arg(long)]
        host: Option<String>,
    },

    /// Manage OS service lifecycle (launchd/systemd user service)
    Service {
        /// Init system to use: auto (detect), systemd, or openrc
        #[arg(long, default_value = "auto", value_parser = ["auto", "systemd", "openrc"])]
        service_init: String,

        #[command(subcommand)]
        service_command: ServiceCommands,
    },

    /// Run diagnostics for daemon/scheduler/channel freshness
    #[command(long_about = "\
Run diagnostics for config, protocol registry, workspace, daemon, and environment.

After the report, a short [maintenance] section explains what can change without \
rebuilding the binary versus when `cargo build` / `cargo install` is required.

Subcommands:
  maintenance   Full operator guide (config, policy, protocol vs rebuild)
  models        Probe live model catalogs across providers
  template-dag  Validate a handwritten CR-L2 template DAG fixture (no LLM)")]
    Doctor {
        #[command(subcommand)]
        doctor_command: Option<DoctorCommands>,
    },

    /// Show system status (full details)
    Status,

    /// Configure and manage scheduled tasks
    #[command(long_about = "\
Configure and manage scheduled tasks.

Schedule recurring, one-shot, or interval-based tasks using cron \
expressions, RFC 3339 timestamps, durations, or fixed intervals.

Cron expressions use the standard 5-field format: \
'min hour day month weekday'. Timezones default to UTC; \
override with --tz and an IANA timezone name.

Examples:
  velaclaw cron list
  velaclaw cron add '0 9 * * 1-5' 'Good morning' --tz America/New_York
  velaclaw cron add '*/30 * * * *' 'Check system health'
  velaclaw cron add-at 2025-01-15T14:00:00Z 'Send reminder'
  velaclaw cron add-every 60000 'Ping heartbeat'
  velaclaw cron once 30m 'Run backup in 30 minutes'
  velaclaw cron pause <task-id>
  velaclaw cron update <task-id> --expression '0 8 * * *' --tz Europe/London")]
    Cron {
        #[command(subcommand)]
        cron_command: CronCommands,
    },

    /// Manage provider model catalogs
    Models {
        #[command(subcommand)]
        model_command: ModelCommands,
    },

    /// List supported AI providers
    Providers,

    /// Manage channels (telegram, discord, slack)
    #[command(long_about = "\
Manage communication channels.

Add, remove, list, and health-check channels that connect VelaClaw \
to messaging platforms. Supported channel types: telegram, discord, \
slack, whatsapp, matrix, imessage, email.

Examples:
  velaclaw channel list
  velaclaw channel doctor
  velaclaw channel add telegram '{\"bot_token\":\"...\",\"name\":\"my-bot\"}'
  velaclaw channel remove my-bot
  velaclaw channel bind-telegram velaclaw_user")]
    Channel {
        #[command(subcommand)]
        channel_command: ChannelCommands,
    },

    /// Browse 50+ integrations
    Integrations {
        #[command(subcommand)]
        integration_command: IntegrationCommands,
    },

    /// Manage skills (user-defined capabilities)
    Skills {
        #[command(subcommand)]
        skill_command: SkillCommands,
    },

    /// Migrate data from other agent runtimes
    Migrate {
        #[command(subcommand)]
        migrate_command: MigrateCommands,
    },

    /// Manage provider subscription authentication profiles
    Auth {
        #[command(subcommand)]
        auth_command: AuthCommands,
    },

    /// Discover and introspect USB hardware
    #[command(long_about = "\
Discover and introspect USB hardware.

Enumerate connected USB devices, identify known development boards \
(STM32 Nucleo, Arduino, ESP32), and retrieve chip information via \
probe-rs / ST-Link.

Examples:
  velaclaw hardware discover
  velaclaw hardware introspect /dev/ttyACM0
  velaclaw hardware info --chip STM32F401RETx")]
    Hardware {
        #[command(subcommand)]
        hardware_command: HardwareCommands,
    },

    /// Manage hardware peripherals (STM32, RPi GPIO, etc.)
    #[command(long_about = "\
Manage hardware peripherals.

Add, list, flash, and configure hardware boards that expose tools \
to the agent (GPIO, sensors, actuators). Supported boards: \
nucleo-f401re, rpi-gpio, esp32, arduino-uno.

Examples:
  velaclaw peripheral list
  velaclaw peripheral add nucleo-f401re /dev/ttyACM0
  velaclaw peripheral add rpi-gpio native
  velaclaw peripheral flash --port /dev/cu.usbmodem12345
  velaclaw peripheral flash-nucleo")]
    Peripheral {
        #[command(subcommand)]
        peripheral_command: PeripheralCommands,
    },

    /// Manage agent memory (list, get, stats, clear)
    #[command(long_about = "\
Manage agent memory entries.

List, inspect, and clear memory entries stored by the agent. \
Supports filtering by category and session, pagination, and \
batch clearing with confirmation.

Examples:
  velaclaw memory stats
  velaclaw memory list
  velaclaw memory list --category core --limit 10
  velaclaw memory get <key>
  velaclaw memory clear --category conversation --yes")]
    Memory {
        #[command(subcommand)]
        memory_command: MemoryCommands,
    },

    /// Manage configuration
    #[command(long_about = "\
Manage VelaClaw configuration.

Inspect and export configuration settings. Use 'schema' to dump \
the full JSON Schema for the config file, which documents every \
available key, type, and default value.

Examples:
  velaclaw config schema              # print JSON Schema to stdout
  velaclaw config schema > schema.json")]
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
    },
    /// Deploy VelaClaw to remote servers
    #[command(long_about = "\
Deploy VelaClaw to remote servers via SSH.

Manage remote deployments with support for direct binary deployment, Docker containers, and systemd services. Includes health checks, status monitoring, rollback, and configuration sync capabilities.

Examples:
  velaclaw deploy deploy --server prod-001
  velaclaw deploy status --server prod-001
  velaclaw deploy health-check --server prod-001
  velaclaw deploy list
  velaclaw deploy rollback --server prod-001")]
    Deploy {
        #[command(subcommand)]
        deploy_command: deploy::DeployCommands,
    },

    /// Generate shell completion script to stdout
    #[command(long_about = "\
Generate shell completion scripts for `velaclaw`.

The script is printed to stdout so it can be sourced directly:

Examples:
  source <(velaclaw completions bash)
  velaclaw completions zsh > ~/.zfunc/_velaclaw
  velaclaw completions fish > ~/.config/fish/completions/velaclaw.fish")]
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Dump the full configuration JSON Schema to stdout
    Schema,
}

#[derive(Subcommand, Debug)]
enum AuthCommands {
    /// Login with OpenAI Codex OAuth
    Login {
        /// Provider (`openai-codex`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Use OAuth device-code flow
        #[arg(long)]
        device_code: bool,
    },
    /// Complete OAuth by pasting redirect URL or auth code
    PasteRedirect {
        /// Provider (`openai-codex`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Full redirect URL or raw OAuth code
        #[arg(long)]
        input: Option<String>,
    },
    /// Paste setup token / auth token (for Anthropic subscription auth)
    PasteToken {
        /// Provider (`anthropic`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Token value (if omitted, read interactively)
        #[arg(long)]
        token: Option<String>,
        /// Auth kind override (`authorization` or `api-key`)
        #[arg(long)]
        auth_kind: Option<String>,
    },
    /// Alias for `paste-token` (interactive by default)
    SetupToken {
        /// Provider (`anthropic`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Refresh OpenAI Codex access token using refresh token
    Refresh {
        /// Provider (`openai-codex`)
        #[arg(long)]
        provider: String,
        /// Profile name or profile id
        #[arg(long)]
        profile: Option<String>,
    },
    /// Remove auth profile
    Logout {
        /// Provider
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Set active profile for a provider
    Use {
        /// Provider
        #[arg(long)]
        provider: String,
        /// Profile name or full profile id
        #[arg(long)]
        profile: String,
    },
    /// List auth profiles
    List,
    /// Show auth status with active profile and token expiry info
    Status,
}

#[derive(Subcommand, Debug)]
enum ModelCommands {
    /// Refresh and cache provider models
    Refresh {
        /// Provider name (defaults to configured default provider)
        #[arg(long)]
        provider: Option<String>,

        /// Force live refresh and ignore fresh cache
        #[arg(long)]
        force: bool,
    },
    /// List provider manifests discovered under `AI_PROTOCOL_DIR`
    ProtocolProviders {
        /// Emit JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// List logical model ids from registries under `AI_PROTOCOL_DIR`
    ProtocolModels {
        /// Emit JSON instead of a table
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum DoctorCommands {
    /// Operator guide: config/policy/protocol changes vs binary rebuild
    Maintenance,

    /// CR-HOST-002: aggregate L4 M3c/d/e fields from local logs (observe-only)
    L4ShadowSummary {
        /// Path to a log file (`-` = stdin). Required.
        #[arg(long)]
        log: String,

        /// Emit JSON aggregate instead of a human table
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Probe model catalogs across providers and report availability
    Models {
        /// Probe a specific provider only (default: all known providers)
        #[arg(long)]
        provider: Option<String>,

        /// Prefer cached catalogs when available (skip forced live refresh)
        #[arg(long)]
        use_cache: bool,
    },

    /// Validate a handwritten CR-L2 template DAG fixture (assemble-only; no LLM)
    TemplateDag {
        /// Path to a schema_version 0.1.0 template DAG JSON fixture
        #[arg(long)]
        fixture: String,

        /// Seed user message passed into each node assemble (diagnostic only)
        #[arg(long, default_value = "doctor template-dag probe")]
        message: String,

        /// Use compact ContextBudget (matches `[agent].compact_context` intent)
        #[arg(long, default_value_t = false)]
        compact: bool,
    },

    /// CR-L4-003: observe candidate DAG validate + L2 fallback (assemble-only; no LLM)
    CandidateDag {
        /// Path to a candidate (model-shaped) DAG JSON
        #[arg(long)]
        candidate: String,

        /// Optional handwritten L2 fallback template path (default: embedded code-fix)
        #[arg(long)]
        fallback: Option<String>,

        /// Seed user message passed into assemble (diagnostic only)
        #[arg(long, default_value = "doctor candidate-dag probe")]
        message: String,

        /// Use compact ContextBudget (matches `[agent].compact_context` intent)
        #[arg(long, default_value_t = false)]
        compact: bool,

        /// Optional output-hash stagnation limit (`0` = off)
        #[arg(long, default_value_t = 0)]
        stagnation_limit: u32,
    },

    /// CR-CAP-002/004: host-local Tag → candidates + reachable (keys ∩ declared)
    Capabilities {
        /// Capability Tag from capability-mapping.md (omit to list all Tag counts)
        #[arg(long)]
        tag: Option<String>,

        /// Force rebuild of `capability-index.json` under the config dir
        #[arg(long, default_value_t = false)]
        rebuild: bool,

        /// With `--tag`, list only providers that have a usable local API key
        #[arg(long, default_value_t = false)]
        reachable_only: bool,
    },

    /// CR-CAP-003/005: observe capability-index route (Tag/Hint → reachable ∩ constraints; no LLM)
    #[command(visible_alias = "capability-route")]
    IntentRoute {
        /// User message used for query_classification (optional if --hint/--tag set)
        #[arg(long, default_value = "doctor capability-route probe")]
        message: String,

        /// Explicit hint (skips classification when set; Tag names also accepted)
        #[arg(long)]
        hint: Option<String>,

        /// Explicit Capability Tag (preferred; skips classifier — CR-CAP-005)
        #[arg(long)]
        tag: Option<String>,

        /// Force rebuild of capability-index.json
        #[arg(long, default_value_t = false)]
        rebuild: bool,

        /// Observe with route logic even when `[agent].intent_capability_route` is false
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Append decision JSONL under `<config_dir>/intent-route-decisions.jsonl` (opt-in)
        #[arg(long, default_value_t = false)]
        persist: bool,
    },

    /// VL-DR-001: explain provider_mode + BYOK effective model (no LLM; no secrets)
    Routing,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install default crypto provider for Rustls TLS.
    // This prevents the error: "could not automatically determine the process-level CryptoProvider"
    // when both aws-lc-rs and ring features are available (or neither is explicitly selected).
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Warning: Failed to install default crypto provider: {e:?}");
    }

    let cli = Cli::parse();

    if let Some(config_dir) = &cli.config_dir {
        if config_dir.trim().is_empty() {
            bail!("--config-dir cannot be empty");
        }
        std::env::set_var("VELACLAW_CONFIG_DIR", config_dir);
    }

    // Completions must remain stdout-only and should not load config or initialize logging.
    // This avoids warnings/log lines corrupting sourced completion scripts.
    if let Commands::Completions { shell } = &cli.command {
        let mut stdout = std::io::stdout().lock();
        write_shell_completion(*shell, &mut stdout)?;
        return Ok(());
    }

    // Maintenance guide is self-help text: work even when config is missing/broken.
    if let Commands::Doctor {
        doctor_command: Some(DoctorCommands::Maintenance),
    } = &cli.command
    {
        velaclaw::doctor::print_maintenance_guide();
        return Ok(());
    }

    // L4 shadow aggregate is observe-only over an explicit log path — no config needed.
    if let Commands::Doctor {
        doctor_command: Some(DoctorCommands::L4ShadowSummary { log, json }),
    } = &cli.command
    {
        let path = if log == "-" {
            std::path::PathBuf::from("-")
        } else {
            std::path::PathBuf::from(log)
        };
        return velaclaw::doctor::run_l4_shadow_summary(Some(path.as_path()), *json);
    }

    // Initialize logging - respects RUST_LOG env var, defaults to INFO
    let subscriber = fmt::Subscriber::builder()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    if let Commands::Onboard {
        interactive,
        force,
        channels_only,
        api_key,
        provider,
        model,
        memory,
    } = &cli.command
    {
        return cli_dispatch::run_onboard_command(
            *interactive,
            *force,
            *channels_only,
            api_key.clone(),
            provider.clone(),
            model.clone(),
            memory.clone(),
        )
        .await;
    }

    // All other commands need config loaded first
    let mut config = Config::load_or_init().await?;
    config.apply_env_overrides();

    Box::pin(cli_dispatch::dispatch_configured_command(
        cli.command,
        config,
    ))
    .await
}

fn write_shell_completion<W: Write>(shell: CompletionShell, writer: &mut W) -> Result<()> {
    use clap_complete::generate;
    use clap_complete::shells;

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();

    match shell {
        CompletionShell::Bash => generate(shells::Bash, &mut cmd, bin_name.clone(), writer),
        CompletionShell::Fish => generate(shells::Fish, &mut cmd, bin_name.clone(), writer),
        CompletionShell::Zsh => generate(shells::Zsh, &mut cmd, bin_name.clone(), writer),
        CompletionShell::PowerShell => {
            generate(shells::PowerShell, &mut cmd, bin_name.clone(), writer);
        }
        CompletionShell::Elvish => generate(shells::Elvish, &mut cmd, bin_name, writer),
    }

    writer.flush()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOpenAiLogin {
    profile: String,
    code_verifier: String,
    state: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOpenAiLoginFile {
    profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_verifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encrypted_code_verifier: Option<String>,
    state: String,
    created_at: String,
}

fn pending_openai_login_path(config: &Config) -> std::path::PathBuf {
    auth::state_dir_from_config(config).join("auth-openai-pending.json")
}

fn pending_openai_secret_store(config: &Config) -> security::secrets::SecretStore {
    security::secrets::SecretStore::new(
        &auth::state_dir_from_config(config),
        config.secrets.encrypt,
    )
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

fn save_pending_openai_login(config: &Config, pending: &PendingOpenAiLogin) -> Result<()> {
    let path = pending_openai_login_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let secret_store = pending_openai_secret_store(config);
    let encrypted_code_verifier = secret_store.encrypt(&pending.code_verifier)?;
    let persisted = PendingOpenAiLoginFile {
        profile: pending.profile.clone(),
        code_verifier: None,
        encrypted_code_verifier: Some(encrypted_code_verifier),
        state: pending.state.clone(),
        created_at: pending.created_at.clone(),
    };
    let tmp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let json = serde_json::to_vec_pretty(&persisted)?;
    std::fs::write(&tmp, json)?;
    set_owner_only_permissions(&tmp)?;
    std::fs::rename(tmp, &path)?;
    set_owner_only_permissions(&path)?;
    Ok(())
}

fn load_pending_openai_login(config: &Config) -> Result<Option<PendingOpenAiLogin>> {
    let path = pending_openai_login_path(config);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let persisted: PendingOpenAiLoginFile = serde_json::from_slice(&bytes)?;
    let secret_store = pending_openai_secret_store(config);
    let code_verifier = if let Some(encrypted) = persisted.encrypted_code_verifier {
        secret_store.decrypt(&encrypted)?
    } else if let Some(plaintext) = persisted.code_verifier {
        plaintext
    } else {
        bail!("Pending OpenAI login is missing code verifier");
    };
    Ok(Some(PendingOpenAiLogin {
        profile: persisted.profile,
        code_verifier,
        state: persisted.state,
        created_at: persisted.created_at,
    }))
}

fn clear_pending_openai_login(config: &Config) {
    let path = pending_openai_login_path(config);
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
        let _ = file.set_len(0);
        let _ = file.sync_all();
    }
    let _ = std::fs::remove_file(path);
}

fn read_auth_input(prompt: &str) -> Result<String> {
    let input = Password::new()
        .with_prompt(prompt)
        .allow_empty_password(false)
        .interact()?;
    Ok(input.trim().to_string())
}

fn read_plain_input(prompt: &str) -> Result<String> {
    let input: String = Input::new().with_prompt(prompt).interact_text()?;
    Ok(input.trim().to_string())
}

fn extract_openai_account_id_for_profile(access_token: &str) -> Option<String> {
    let account_id = auth::openai_oauth::extract_account_id_from_jwt(access_token);
    if account_id.is_none() {
        warn!(
            "Could not extract OpenAI account id from OAuth access token; \
             requests may fail until re-authentication."
        );
    }
    account_id
}

fn format_expiry(profile: &auth::profiles::AuthProfile) -> String {
    match profile
        .token_set
        .as_ref()
        .and_then(|token_set| token_set.expires_at)
    {
        Some(ts) => {
            let now = chrono::Utc::now();
            if ts <= now {
                format!("expired at {}", ts.to_rfc3339())
            } else {
                let mins = (ts - now).num_minutes();
                format!("expires in {mins}m ({})", ts.to_rfc3339())
            }
        }
        None => "n/a".to_string(),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn handle_auth_command(auth_command: AuthCommands, config: &Config) -> Result<()> {
    let auth_service = auth::AuthService::from_config(config);

    match auth_command {
        AuthCommands::Login {
            provider,
            profile,
            device_code,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            if provider != "openai-codex" {
                bail!("`auth login` currently supports only --provider openai-codex");
            }

            let client = reqwest::Client::new();

            if device_code {
                match auth::openai_oauth::start_device_code_flow(&client).await {
                    Ok(device) => {
                        println!("OpenAI device-code login started.");
                        println!("Visit: {}", device.verification_uri);
                        println!("Code:  {}", device.user_code);
                        if let Some(uri_complete) = &device.verification_uri_complete {
                            println!("Fast link: {uri_complete}");
                        }
                        if let Some(message) = &device.message {
                            println!("{message}");
                        }

                        let token_set =
                            auth::openai_oauth::poll_device_code_tokens(&client, &device).await?;
                        let account_id =
                            extract_openai_account_id_for_profile(&token_set.access_token);

                        auth_service
                            .store_openai_tokens(&profile, token_set, account_id, true)
                            .await?;
                        clear_pending_openai_login(config);

                        println!("Saved profile {profile}");
                        println!("Active profile for openai-codex: {profile}");
                        return Ok(());
                    }
                    Err(e) => {
                        println!(
                            "Device-code flow unavailable: {e}. Falling back to browser/paste flow."
                        );
                    }
                }
            }

            let pkce = auth::openai_oauth::generate_pkce_state();
            let pending = PendingOpenAiLogin {
                profile: profile.clone(),
                code_verifier: pkce.code_verifier.clone(),
                state: pkce.state.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            save_pending_openai_login(config, &pending)?;

            let authorize_url = auth::openai_oauth::build_authorize_url(&pkce);
            println!("Open this URL in your browser and authorize access:");
            println!("{authorize_url}");
            println!();
            println!("Waiting for callback at http://localhost:1455/auth/callback ...");

            let code = match auth::openai_oauth::receive_loopback_code(
                &pkce.state,
                std::time::Duration::from_secs(180),
            )
            .await
            {
                Ok(code) => code,
                Err(e) => {
                    println!("Callback capture failed: {e}");
                    println!(
                            "Run `velaclaw auth paste-redirect --provider openai-codex --profile {profile}`"
                        );
                    return Ok(());
                }
            };

            let token_set =
                auth::openai_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
            let account_id = extract_openai_account_id_for_profile(&token_set.access_token);

            auth_service
                .store_openai_tokens(&profile, token_set, account_id, true)
                .await?;
            clear_pending_openai_login(config);

            println!("Saved profile {profile}");
            println!("Active profile for openai-codex: {profile}");
            Ok(())
        }

        AuthCommands::PasteRedirect {
            provider,
            profile,
            input,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            if provider != "openai-codex" {
                bail!("`auth paste-redirect` currently supports only --provider openai-codex");
            }

            let pending = load_pending_openai_login(config)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "No pending OpenAI login found. Run `velaclaw auth login --provider openai-codex` first."
                )
            })?;

            if pending.profile != profile {
                bail!(
                    "Pending login profile mismatch: pending={}, requested={}",
                    pending.profile,
                    profile
                );
            }

            let redirect_input = match input {
                Some(value) => value,
                None => read_plain_input("Paste redirect URL or OAuth code")?,
            };

            let code = auth::openai_oauth::parse_code_from_redirect(
                &redirect_input,
                Some(&pending.state),
            )?;

            let pkce = auth::openai_oauth::PkceState {
                code_verifier: pending.code_verifier.clone(),
                code_challenge: String::new(),
                state: pending.state.clone(),
            };

            let client = reqwest::Client::new();
            let token_set =
                auth::openai_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
            let account_id = extract_openai_account_id_for_profile(&token_set.access_token);

            auth_service
                .store_openai_tokens(&profile, token_set, account_id, true)
                .await?;
            clear_pending_openai_login(config);

            println!("Saved profile {profile}");
            println!("Active profile for openai-codex: {profile}");
            Ok(())
        }

        AuthCommands::PasteToken {
            provider,
            profile,
            token,
            auth_kind,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            let token = match token {
                Some(token) => token.trim().to_string(),
                None => read_auth_input("Paste token")?,
            };
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, auth_kind.as_deref());
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_provider_token(&provider, &profile, &token, metadata, true)
                .await?;
            println!("Saved profile {profile}");
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::SetupToken { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            let token = read_auth_input("Paste token")?;
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, Some("authorization"));
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_provider_token(&provider, &profile, &token, metadata, true)
                .await?;
            println!("Saved profile {profile}");
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::Refresh { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            if provider != "openai-codex" {
                bail!("`auth refresh` currently supports only --provider openai-codex");
            }

            match auth_service
                .get_valid_openai_access_token(profile.as_deref())
                .await?
            {
                Some(_) => {
                    println!("OpenAI Codex token is valid (refresh completed if needed).");
                    Ok(())
                }
                None => {
                    bail!(
                        "No OpenAI Codex auth profile found. Run `velaclaw auth login --provider openai-codex`."
                    )
                }
            }
        }

        AuthCommands::Logout { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            let removed = auth_service.remove_profile(&provider, &profile).await?;
            if removed {
                println!("Removed auth profile {provider}:{profile}");
            } else {
                println!("Auth profile not found: {provider}:{profile}");
            }
            Ok(())
        }

        AuthCommands::Use { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            auth_service.set_active_profile(&provider, &profile).await?;
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::List => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("No auth profiles configured.");
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!("{marker} {id}");
            }

            Ok(())
        }

        AuthCommands::Status => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("No auth profiles configured.");
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!(
                    "{} {} kind={:?} account={} expires={}",
                    marker,
                    id,
                    profile.kind,
                    crate::security::redact(profile.account_id.as_deref().unwrap_or("unknown")),
                    format_expiry(profile)
                );
            }

            println!();
            println!("Active profiles:");
            for (provider, profile_id) in &data.active_profiles {
                println!("  {provider}: {profile_id}");
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};
    use velaclaw::DEFAULT_PROTOCOL_MODEL_ID;

    #[test]
    fn cli_definition_has_no_flag_conflicts() {
        Cli::command().debug_assert();
    }

    #[test]
    fn onboard_help_includes_model_flag() {
        let cmd = Cli::command();
        let onboard = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "onboard")
            .expect("onboard subcommand must exist");

        let has_model_flag = onboard
            .get_arguments()
            .any(|arg| arg.get_id().as_str() == "model" && arg.get_long() == Some("model"));

        assert!(
            has_model_flag,
            "onboard help should include --model for quick setup overrides"
        );
    }

    #[test]
    fn onboard_cli_accepts_model_provider_and_api_key_in_quick_mode() {
        let cli = Cli::try_parse_from([
            "velaclaw",
            "onboard",
            "--provider",
            DEFAULT_PROTOCOL_MODEL_ID,
            "--model",
            "custom-model-946",
            "--api-key",
            "sk-issue946",
        ])
        .expect("quick onboard invocation should parse");

        match cli.command {
            Commands::Onboard {
                interactive,
                force,
                channels_only,
                api_key,
                provider,
                model,
                ..
            } => {
                assert!(!interactive);
                assert!(!force);
                assert!(!channels_only);
                assert_eq!(provider.as_deref(), Some(DEFAULT_PROTOCOL_MODEL_ID));
                assert_eq!(model.as_deref(), Some("custom-model-946"));
                assert_eq!(api_key.as_deref(), Some("sk-issue946"));
            }
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    fn completions_cli_parses_supported_shells() {
        for shell in ["bash", "fish", "zsh", "powershell", "elvish"] {
            let cli = Cli::try_parse_from(["velaclaw", "completions", shell])
                .expect("completions invocation should parse");
            match cli.command {
                Commands::Completions { .. } => {}
                other => panic!("expected completions command, got {other:?}"),
            }
        }
    }

    #[test]
    fn completion_generation_mentions_binary_name() {
        let mut output = Vec::new();
        write_shell_completion(CompletionShell::Bash, &mut output)
            .expect("completion generation should succeed");
        let script = String::from_utf8(output).expect("completion output should be valid utf-8");
        assert!(
            script.contains("velaclaw"),
            "completion script should reference binary name"
        );
    }

    #[test]
    fn onboard_cli_accepts_force_flag() {
        let cli = Cli::try_parse_from(["velaclaw", "onboard", "--force"])
            .expect("onboard --force should parse");

        match cli.command {
            Commands::Onboard { force, .. } => assert!(force),
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    fn agent_cli_accepts_no_color_and_no_fold_flags() {
        let cli = Cli::try_parse_from([
            "velaclaw",
            "agent",
            "--no-color",
            "--no-fold",
            "-m",
            "hello",
        ])
        .expect("agent flags should parse");

        match cli.command {
            Commands::Agent {
                message,
                no_color,
                no_fold,
                ..
            } => {
                assert_eq!(message.as_deref(), Some("hello"));
                assert!(no_color);
                assert!(no_fold);
            }
            other => panic!("expected agent command, got {other:?}"),
        }
    }

    #[test]
    fn agent_cli_flags_default_off() {
        let cli = Cli::try_parse_from(["velaclaw", "agent"]).expect("agent defaults");
        match cli.command {
            Commands::Agent {
                no_color, no_fold, ..
            } => {
                assert!(!no_color);
                assert!(!no_fold);
            }
            other => panic!("expected agent command, got {other:?}"),
        }
    }
}
