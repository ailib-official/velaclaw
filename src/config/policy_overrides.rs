//! L2.5 `.velaclaw/policy-overrides.yaml` I/O and persistence (VL-SEC-004 / VL-UR-004).
//! App 层读写工作区策略覆盖文件；schema/合并逻辑在 `velaclaw-config`。

use super::Config;
use anyhow::{Context, Result};
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
