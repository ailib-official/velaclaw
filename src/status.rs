//! CLI `velaclaw status` display (VL-REVIEW-004).

use crate::config::{Config, DEFAULT_PROTOCOL_MODEL_ID};
use crate::memory;
use anyhow::Result;

/// Print a human-readable runtime status summary for the loaded config.
pub fn print_status(config: &Config) -> Result<()> {
    println!("🦀 VelaClaw Status");
    println!();
    println!("Version:     {}", env!("CARGO_PKG_VERSION"));
    println!("Workspace:   {}", config.workspace_dir.display());
    println!("Config:      {}", config.config_path.display());
    println!();
    println!(
        "🤖 Provider:      {}",
        config
            .default_provider
            .as_deref()
            .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID)
    );
    println!(
        "   Model:         {}",
        config.default_model.as_deref().unwrap_or("(default)")
    );
    println!("📊 Observability:  {}", config.observability.backend);
    println!("🛡️  Autonomy:      {:?}", config.autonomy.level);
    println!("⚙️  Runtime:       {}", config.runtime.kind);
    let effective_memory_backend = memory::effective_memory_backend_name(
        &config.memory.backend,
        Some(&config.storage.provider.config),
    );
    println!(
        "💓 Heartbeat:      {}",
        if config.heartbeat.enabled {
            format!("every {}min", config.heartbeat.interval_minutes)
        } else {
            "disabled".into()
        }
    );
    println!(
        "🧠 Memory:         {} (auto-save: {})",
        effective_memory_backend,
        if config.memory.auto_save { "on" } else { "off" }
    );

    println!();
    println!("Security:");
    println!("  Workspace only:    {}", config.autonomy.workspace_only);
    println!(
        "  Allowed commands:  {}",
        config.autonomy.allowed_commands.join(", ")
    );
    println!(
        "  Max actions/hour:  {}",
        config.autonomy.max_actions_per_hour
    );
    println!(
        "  Max cost/day:      ${:.2}",
        f64::from(config.autonomy.max_cost_per_day_cents) / 100.0
    );
    println!();
    println!("Channels:");
    println!("  CLI:      ✅ always");
    for (name, configured) in [
        ("Telegram", config.channels_config.telegram.is_some()),
        ("Discord", config.channels_config.discord.is_some()),
        ("Slack", config.channels_config.slack.is_some()),
        ("Webhook", config.channels_config.webhook.is_some()),
        ("Nextcloud", config.channels_config.nextcloud_talk.is_some()),
    ] {
        println!(
            "  {name:9} {}",
            if configured {
                "✅ configured"
            } else {
                "❌ not configured"
            }
        );
    }
    println!();
    println!("Peripherals:");
    println!(
        "  Enabled:   {}",
        if config.peripherals.enabled {
            "yes"
        } else {
            "no"
        }
    );
    println!("  Boards:    {}", config.peripherals.boards.len());

    Ok(())
}
