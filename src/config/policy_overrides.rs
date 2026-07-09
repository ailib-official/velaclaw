//! L2.5 `.velaclaw/policy-overrides.yaml` I/O and persistence (VL-SEC-004 / VL-UR-004).
//! App 层读写工作区策略覆盖文件；schema/合并逻辑在 `velaclaw-config`。

use super::Config;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use velaclaw_config::{
    discover_and_load_policy_overrides, load_policy_overrides_from_path, policy_overrides_path,
    reject_forbidden_secret_keys, AgentPolicyLayer, PolicyOverridesLayer, SelfAdjustEnforcer,
};

/// Workspace-scoped store for persisting operator "Always" decisions.
#[derive(Debug)]
pub struct PolicyOverridesStore {
    workspace_dir: PathBuf,
    enforcer: SelfAdjustEnforcer,
}

impl PolicyOverridesStore {
    pub fn new(config: &Config, l2: Option<&AgentPolicyLayer>) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            enforcer: SelfAdjustEnforcer::from_self_adjust(l2.and_then(|p| p.self_adjust.as_ref())),
        }
    }

    pub fn from_workspace(workspace_dir: PathBuf, l2: Option<&AgentPolicyLayer>) -> Self {
        Self {
            workspace_dir,
            enforcer: SelfAdjustEnforcer::from_self_adjust(l2.and_then(|p| p.self_adjust.as_ref())),
        }
    }

    pub fn path(&self) -> PathBuf {
        policy_overrides_path(&self.workspace_dir)
    }

    pub fn load(&self) -> Result<Option<PolicyOverridesLayer>> {
        discover_and_load_policy_overrides(&self.workspace_dir)
    }

    /// Apply one dot-path patch to L2.5 after `self_adjust` validation.
    pub fn apply_patch(&self, patch_path: &str, value: Value) -> Result<PathBuf> {
        self.enforcer.validate_write_path(patch_path)?;
        let path = self.path();
        let mut layer = load_policy_overrides_from_path(&path)?.unwrap_or_default();
        layer.version = Some(1);
        apply_dot_patch(&mut layer, patch_path, &value)?;
        save_policy_overrides(&path, &layer)?;
        Ok(path)
    }

    /// Append `tool_name` to `approval.session_allowlist` and atomically persist.
    pub fn persist_session_allowlist_add(&self, tool_name: &str) -> Result<()> {
        self.enforcer.validate_session_allowlist_tool(tool_name)?;
        let path = self.path();
        let mut layer = load_policy_overrides_from_path(&path)?.unwrap_or_default();
        layer.version = Some(1);
        let approval = layer.approval.get_or_insert_with(Default::default);
        if !approval
            .session_allowlist
            .iter()
            .any(|existing| existing == tool_name)
        {
            approval.session_allowlist.push(tool_name.to_string());
            approval.session_allowlist.sort();
            approval.session_allowlist.dedup();
        }
        save_policy_overrides(&path, &layer)
    }
}

pub fn discover_policy_overrides(config: &Config) -> Result<Option<PolicyOverridesLayer>> {
    discover_and_load_policy_overrides(&config.workspace_dir)
        .with_context(|| "load workspace policy-overrides.yaml")
}

fn apply_dot_patch(
    layer: &mut PolicyOverridesLayer,
    patch_path: &str,
    value: &Value,
) -> Result<()> {
    match patch_path {
        "approval.session_allowlist" => {
            let approval = layer.approval.get_or_insert_with(Default::default);
            approval.session_allowlist = parse_string_list(value, patch_path)?;
        }
        "autonomy.level" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.level = Some(parse_string(value, patch_path)?);
        }
        "autonomy.workspace_only" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.workspace_only = Some(parse_bool(value, patch_path)?);
        }
        "autonomy.allowed_commands" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.allowed_commands = Some(parse_string_list(value, patch_path)?);
        }
        "autonomy.forbidden_paths" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.forbidden_paths = Some(parse_string_list(value, patch_path)?);
        }
        "autonomy.max_actions_per_hour" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.max_actions_per_hour = Some(parse_u32(value, patch_path)?);
        }
        "autonomy.max_cost_per_day_cents" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.max_cost_per_day_cents = Some(parse_u32(value, patch_path)?);
        }
        "autonomy.require_approval_for_medium_risk" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.require_approval_for_medium_risk = Some(parse_bool(value, patch_path)?);
        }
        "autonomy.block_high_risk_commands" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.block_high_risk_commands = Some(parse_bool(value, patch_path)?);
        }
        "autonomy.auto_approve" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.auto_approve = Some(parse_string_list(value, patch_path)?);
        }
        "autonomy.always_ask" => {
            let autonomy = layer.autonomy.get_or_insert_with(Default::default);
            autonomy.always_ask = Some(parse_string_list(value, patch_path)?);
        }
        other => bail!("unsupported policy patch path: {other}"),
    }
    Ok(())
}

fn parse_string(value: &Value, field: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("{field} must be a string"))
}

fn parse_bool(value: &Value, field: &str) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| anyhow::anyhow!("{field} must be a boolean"))
}

fn parse_u32(value: &Value, field: &str) -> Result<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| anyhow::anyhow!("{field} must be a non-negative integer"))
}

fn parse_string_list(value: &Value, field: &str) -> Result<Vec<String>> {
    let Some(items) = value.as_array() else {
        bail!("{field} must be a JSON array of strings");
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(
            item.as_str()
                .ok_or_else(|| anyhow::anyhow!("{field} must be a JSON array of strings"))?
                .to_string(),
        );
    }
    Ok(out)
}

pub fn save_policy_overrides(path: &Path, layer: &PolicyOverridesLayer) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let content = serde_yaml::to_string(layer).context("serialize policy-overrides.yaml")?;
    reject_forbidden_secret_keys(&content)?;
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, &content).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use velaclaw_config::POLICY_OVERRIDES_DIR;

    #[test]
    fn persist_session_allowlist_survives_reload() {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        let store = PolicyOverridesStore::new(&config, None);

        store.persist_session_allowlist_add("file_write").unwrap();

        let path = dir
            .path()
            .join(POLICY_OVERRIDES_DIR)
            .join("policy-overrides.yaml");
        assert!(path.is_file());

        let reloaded = load_policy_overrides_from_path(&path).unwrap().unwrap();
        assert_eq!(
            reloaded.approval.unwrap().session_allowlist,
            vec!["file_write".to_string()]
        );
    }

    #[test]
    fn persist_rejects_secret_fields_on_save() {
        let dir = TempDir::new().unwrap();
        let path = policy_overrides_path(dir.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let layer = PolicyOverridesLayer {
            version: Some(1),
            approval: None,
            autonomy: None,
        };
        save_policy_overrides(&path, &layer).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn l25_merged_on_resolve_effective_autonomy() {
        use super::super::resolve_effective_autonomy;

        let dir = TempDir::new().unwrap();
        let overrides_dir = dir.path().join(POLICY_OVERRIDES_DIR);
        fs::create_dir_all(&overrides_dir).unwrap();
        fs::write(
            overrides_dir.join("policy-overrides.yaml"),
            "version: 1\napproval:\n  session_allowlist:\n    - file_write\n",
        )
        .unwrap();

        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();

        let effective = resolve_effective_autonomy(&config).unwrap();
        assert!(effective.auto_approve.contains(&"file_write".to_string()));
    }
}
