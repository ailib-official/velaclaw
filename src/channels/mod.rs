//! 通道子系统，对接 Telegram、Discord 等第三方平台。
//! Channel subsystem for messaging platform integrations.
//!
//! This module provides the multi-channel messaging infrastructure that connects
//! VelaClaw to external platforms. Each channel implements the [`Channel`] trait
//! defined in [`traits`], which provides a uniform interface for sending messages,
//! listening for incoming messages, health checking, and typing indicators.
//!
//! Channels are instantiated by [`start_channels`] based on the runtime configuration.
//! The subsystem manages per-sender conversation history, concurrent message processing
//! with configurable parallelism, and exponential-backoff reconnection for resilience.
//!
//! # Extension
//!
//! To add a new channel, implement [`Channel`] in a new submodule and wire it into
//! [`start_channels`]. See `AGENTS.md` §7.2 for the full change playbook.

pub mod cli;
pub mod dingtalk;
pub mod discord;
mod doctor;
pub mod email_channel;
pub mod imessage;
pub mod irc;
#[cfg(feature = "channel-lark")]
pub mod lark;
pub mod linq;
#[cfg(feature = "channel-matrix")]
pub mod matrix;
pub mod mattermost;
pub mod nextcloud_talk;
pub mod prompt;
pub mod qq;
mod runtime;
pub mod signal;
pub mod slack;
pub mod telegram;
pub mod traits;
pub(crate) use runtime::*;
mod dispatch;
#[cfg(test)]
pub(crate) use dispatch::{
    process_channel_message, run_message_dispatch_loop, spawn_supervised_listener,
    spawn_supervised_listener_with_health_interval,
};
mod start;
pub mod whatsapp;
#[cfg(feature = "whatsapp-web")]
pub mod whatsapp_storage;
#[cfg(feature = "whatsapp-web")]
pub mod whatsapp_web;

pub use cli::CliChannel;
pub use dingtalk::DingTalkChannel;
pub use discord::DiscordChannel;
pub use doctor::doctor_channels;
#[cfg(test)]
pub(crate) use doctor::{classify_health_result, ChannelHealthState};
pub use email_channel::EmailChannel;
pub use imessage::IMessageChannel;
pub use irc::IrcChannel;
#[cfg(feature = "channel-lark")]
pub use lark::LarkChannel;
pub use linq::LinqChannel;
#[cfg(feature = "channel-matrix")]
pub use matrix::MatrixChannel;
pub use mattermost::MattermostChannel;
pub use nextcloud_talk::NextcloudTalkChannel;
pub use prompt::{build_system_prompt, build_system_prompt_pyramid, build_system_prompt_with_mode};
pub use qq::QQChannel;
pub use signal::SignalChannel;
pub use slack::SlackChannel;
pub use start::start_channels;
pub use telegram::TelegramChannel;
pub use traits::{Channel, SendMessage};
pub use whatsapp::WhatsAppChannel;
#[cfg(feature = "whatsapp-web")]
pub use whatsapp_web::WhatsAppWebChannel;

#[cfg(feature = "ai-protocol")]
use crate::agent::dispatcher::ToolDispatcher;
use crate::agent::loop_::{
    append_text_tool_prompt, build_tool_instructions, run_tool_call_loop, SoftFailLoopCtx,
};
use crate::approval::{ApprovalManager, ChannelApprovalHub, ChannelApprovalSession};
use crate::cli_render::{RenderOpts, RenderStyle};
use crate::config::{ChannelApprovalMode, Config, DEFAULT_PROTOCOL_MODEL_ID};
use crate::memory::{self, Memory};
use crate::observability::{self, Observer};
use crate::providers::{self, ChatMessage, Provider};
use crate::runtime as runtime_adapter;
use crate::security::PolicyHandle;
use crate::tools::{self, Tool};
use crate::util::truncate_with_ellipsis;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio_util::sync::CancellationToken;

/// Per-sender conversation history for channel messages.
fn normalize_telegram_identity(value: &str) -> String {
    value.trim().trim_start_matches('@').to_string()
}

async fn bind_telegram_identity(config: &Config, identity: &str) -> Result<()> {
    let normalized = normalize_telegram_identity(identity);
    if normalized.is_empty() {
        anyhow::bail!("Telegram identity cannot be empty");
    }

    let mut updated = config.clone();
    let Some(telegram) = updated.channels_config.telegram.as_mut() else {
        anyhow::bail!(
            "Telegram channel is not configured. Run `velaclaw onboard --channels-only` first"
        );
    };

    if telegram.allowed_users.iter().any(|u| u == "*") {
        println!(
            "⚠️ Telegram allowlist is currently wildcard (`*`) — binding is unnecessary until you remove '*'."
        );
    }

    if telegram
        .allowed_users
        .iter()
        .map(|entry| normalize_telegram_identity(entry))
        .any(|entry| entry == normalized)
    {
        println!("✅ Telegram identity already bound: {normalized}");
        return Ok(());
    }

    telegram.allowed_users.push(normalized.clone());
    updated.save().await?;
    println!("✅ Bound Telegram identity: {normalized}");
    println!("   Saved to {}", updated.config_path.display());
    match maybe_restart_managed_daemon_service() {
        Ok(true) => {
            println!("🔄 Detected running managed daemon service; reloaded automatically.");
        }
        Ok(false) => {
            println!(
                "ℹ️ No managed daemon service detected. If `velaclaw daemon`/`channel start` is already running, restart it to load the updated allowlist."
            );
        }
        Err(e) => {
            eprintln!(
                "⚠️ Allowlist saved, but failed to reload daemon service automatically: {e}\n\
                 Restart service manually with `velaclaw service stop && velaclaw service start`."
            );
        }
    }
    Ok(())
}

fn maybe_restart_managed_daemon_service() -> Result<bool> {
    if cfg!(target_os = "macos") {
        let home = directories::UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .context("Could not find home directory")?;
        let plist = home
            .join("Library")
            .join("LaunchAgents")
            .join("com.velaclaw.daemon.plist");
        if !plist.exists() {
            return Ok(false);
        }

        let list_output = Command::new("launchctl")
            .arg("list")
            .output()
            .context("Failed to query launchctl list")?;
        let listed = String::from_utf8_lossy(&list_output.stdout);
        if !listed.contains("com.velaclaw.daemon") {
            return Ok(false);
        }

        let _ = Command::new("launchctl")
            .args(["stop", "com.velaclaw.daemon"])
            .output();
        let start_output = Command::new("launchctl")
            .args(["start", "com.velaclaw.daemon"])
            .output()
            .context("Failed to start launchd daemon service")?;
        if !start_output.status.success() {
            let stderr = String::from_utf8_lossy(&start_output.stderr);
            anyhow::bail!("launchctl start failed: {}", stderr.trim());
        }

        return Ok(true);
    }

    if cfg!(target_os = "linux") {
        // OpenRC (system-wide) takes precedence over systemd (user-level)
        let openrc_init_script = PathBuf::from("/etc/init.d/velaclaw");
        if openrc_init_script.exists() {
            if let Ok(status_output) = Command::new("rc-service").args(OPENRC_STATUS_ARGS).output()
            {
                // rc-service exits 0 if running, non-zero otherwise
                if status_output.status.success() {
                    let restart_output = Command::new("rc-service")
                        .args(OPENRC_RESTART_ARGS)
                        .output()
                        .context("Failed to restart OpenRC daemon service")?;
                    if !restart_output.status.success() {
                        let stderr = String::from_utf8_lossy(&restart_output.stderr);
                        anyhow::bail!("rc-service restart failed: {}", stderr.trim());
                    }
                    return Ok(true);
                }
            }
        }

        // Systemd (user-level)
        let home = directories::UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .context("Could not find home directory")?;
        let unit_path: PathBuf = home
            .join(".config")
            .join("systemd")
            .join("user")
            .join("velaclaw.service");
        if !unit_path.exists() {
            return Ok(false);
        }

        let active_output = Command::new("systemctl")
            .args(SYSTEMD_STATUS_ARGS)
            .output()
            .context("Failed to query systemd service state")?;
        let state = String::from_utf8_lossy(&active_output.stdout);
        if !state.trim().eq_ignore_ascii_case("active") {
            return Ok(false);
        }

        let restart_output = Command::new("systemctl")
            .args(SYSTEMD_RESTART_ARGS)
            .output()
            .context("Failed to restart systemd daemon service")?;
        if !restart_output.status.success() {
            let stderr = String::from_utf8_lossy(&restart_output.stderr);
            anyhow::bail!("systemctl restart failed: {}", stderr.trim());
        }

        return Ok(true);
    }

    Ok(false)
}

pub async fn handle_command(command: crate::ChannelCommands, config: &Config) -> Result<()> {
    match command {
        crate::ChannelCommands::Start => {
            anyhow::bail!("Start must be handled in main.rs (requires async runtime)")
        }
        crate::ChannelCommands::Doctor => {
            anyhow::bail!("Doctor must be handled in main.rs (requires async runtime)")
        }
        crate::ChannelCommands::List => {
            println!("Channels:");
            println!("  ✅ CLI (always available)");
            for (name, configured) in [
                ("Telegram", config.channels_config.telegram.is_some()),
                ("Discord", config.channels_config.discord.is_some()),
                ("Slack", config.channels_config.slack.is_some()),
                ("Mattermost", config.channels_config.mattermost.is_some()),
                ("Webhook", config.channels_config.webhook.is_some()),
                ("iMessage", config.channels_config.imessage.is_some()),
                (
                    "Matrix",
                    cfg!(feature = "channel-matrix") && config.channels_config.matrix.is_some(),
                ),
                ("Signal", config.channels_config.signal.is_some()),
                ("WhatsApp", config.channels_config.whatsapp.is_some()),
                ("Linq", config.channels_config.linq.is_some()),
                (
                    "Nextcloud Talk",
                    config.channels_config.nextcloud_talk.is_some(),
                ),
                ("Email", config.channels_config.email.is_some()),
                ("IRC", config.channels_config.irc.is_some()),
                (
                    "Lark",
                    cfg!(feature = "channel-lark") && config.channels_config.lark.is_some(),
                ),
                ("DingTalk", config.channels_config.dingtalk.is_some()),
                ("QQ", config.channels_config.qq.is_some()),
            ] {
                println!("  {} {name}", if configured { "✅" } else { "❌" });
            }
            if !cfg!(feature = "channel-matrix") {
                println!(
                    "  ℹ️ Matrix channel support is disabled in this build (enable `channel-matrix`)."
                );
            }
            if !cfg!(feature = "channel-lark") {
                println!(
                    "  ℹ️ Lark channel support is disabled in this build (enable `channel-lark`)."
                );
            }
            println!("\nTo start channels: velaclaw channel start");
            println!("To check health:    velaclaw channel doctor");
            println!("To configure:      velaclaw onboard");
            Ok(())
        }
        crate::ChannelCommands::Add {
            channel_type,
            config: _,
        } => {
            anyhow::bail!(
                "Channel type '{channel_type}' — use `velaclaw onboard` to configure channels"
            );
        }
        crate::ChannelCommands::Remove { name } => {
            anyhow::bail!("Remove channel '{name}' — edit ~/.velaclaw/config.toml directly");
        }
        crate::ChannelCommands::BindTelegram { identity } => {
            bind_telegram_identity(config, &identity).await
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
