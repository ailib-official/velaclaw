//! Effective execution policy — L1 config.toml + L2 agent-policy.yaml merge (VL-SEC-001).
//! 主 crate 桥接：AutonomyConfig ↔ velaclaw-config 分层合并。

use super::{discover_and_load, AutonomyConfig, Config};
use crate::security::AutonomyLevel;
use anyhow::{Context, Result};
use velaclaw_config::{AutonomyLayerValues, EffectiveExecutionPolicy};

/// Resolve merged `[autonomy]` after L1 `config.toml` + L2 `agent-policy.yaml`.
pub fn resolve_effective_autonomy(config: &Config) -> Result<AutonomyConfig> {
    let l1 = autonomy_config_to_layer(&config.autonomy);
    let l2 = discover_and_load(config).with_context(|| "load workspace agent-policy.yaml")?;
    let effective = EffectiveExecutionPolicy::resolve(l1, l2.as_ref());
    layer_to_autonomy_config(&effective.autonomy)
}

fn autonomy_config_to_layer(cfg: &AutonomyConfig) -> AutonomyLayerValues {
    AutonomyLayerValues {
        level: autonomy_level_to_str(cfg.level),
        workspace_only: cfg.workspace_only,
        allowed_commands: cfg.allowed_commands.clone(),
        forbidden_paths: cfg.forbidden_paths.clone(),
        max_actions_per_hour: cfg.max_actions_per_hour,
        max_cost_per_day_cents: cfg.max_cost_per_day_cents,
        require_approval_for_medium_risk: cfg.require_approval_for_medium_risk,
        block_high_risk_commands: cfg.block_high_risk_commands,
        auto_approve: cfg.auto_approve.clone(),
        always_ask: cfg.always_ask.clone(),
    }
}

fn layer_to_autonomy_config(layer: &AutonomyLayerValues) -> Result<AutonomyConfig> {
    Ok(AutonomyConfig {
        level: parse_autonomy_level(&layer.level)?,
        workspace_only: layer.workspace_only,
        allowed_commands: layer.allowed_commands.clone(),
        forbidden_paths: layer.forbidden_paths.clone(),
        max_actions_per_hour: layer.max_actions_per_hour,
        max_cost_per_day_cents: layer.max_cost_per_day_cents,
        require_approval_for_medium_risk: layer.require_approval_for_medium_risk,
        block_high_risk_commands: layer.block_high_risk_commands,
        auto_approve: layer.auto_approve.clone(),
        always_ask: layer.always_ask.clone(),
    })
}

fn autonomy_level_to_str(level: AutonomyLevel) -> String {
    match level {
        AutonomyLevel::ReadOnly => "readonly".into(),
        AutonomyLevel::Supervised => "supervised".into(),
        AutonomyLevel::Full => "full".into(),
    }
}

fn parse_autonomy_level(raw: &str) -> Result<AutonomyLevel> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "readonly" | "read_only" => Ok(AutonomyLevel::ReadOnly),
        "supervised" => Ok(AutonomyLevel::Supervised),
        "full" => Ok(AutonomyLevel::Full),
        other => anyhow::bail!("unsupported autonomy level in agent-policy.yaml: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use velaclaw_config::agent_policy::AGENT_POLICY_FILE;

    #[test]
    fn l2_workspace_policy_overrides_allowed_commands() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(AGENT_POLICY_FILE),
            r#"
version: 2
autonomy:
  allowed_commands: ["echo"]
"#,
        )
        .unwrap();

        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        config.autonomy.allowed_commands = vec!["ls".into(), "cat".into()];

        let effective = resolve_effective_autonomy(&config).unwrap();
        assert_eq!(effective.allowed_commands, vec!["echo"]);
        assert_eq!(effective.level, AutonomyLevel::Supervised);
    }
}
