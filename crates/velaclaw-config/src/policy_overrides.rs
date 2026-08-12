//! L2.5 workspace `.velaclaw/policy-overrides.yaml` schema and merge helpers (VL-SEC-004).
//! 用户可编辑的持久化策略层：session allowlist + 受 self_adjust 约束的 autonomy 补丁。

use crate::agent_policy::{reject_forbidden_secret_keys, AutonomyPolicySection, SelfAdjustSection};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const POLICY_OVERRIDES_DIR: &str = ".velaclaw";
pub const POLICY_OVERRIDES_FILE: &str = "policy-overrides.yaml";

/// Parsed L2.5 `policy-overrides.yaml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyOverridesLayer {
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub approval: Option<ApprovalOverridesSection>,
    #[serde(default)]
    pub autonomy: Option<AutonomyPolicySection>,
}

/// Approval-related persistent overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalOverridesSection {
    /// Tools the operator marked "Always" — merged into L1 `auto_approve` at load.
    #[serde(default)]
    pub session_allowlist: Vec<String>,
    /// Shell executable basenames from shell-policy "Always" (VL-SEC-009).
    /// Not merged into `auto_approve`; hydrated into runtime session state.
    #[serde(default)]
    pub session_shell_binaries: Vec<String>,
}

/// Enforces `self_adjust.allowed_writes` / `denied_writes` for policy patch paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfAdjustEnforcer {
    allowed_writes: Vec<String>,
    denied_writes: Vec<String>,
}

impl SelfAdjustEnforcer {
    pub fn from_self_adjust(section: Option<&SelfAdjustSection>) -> Self {
        if let Some(s) = section {
            Self {
                allowed_writes: s.allowed_writes.clone(),
                denied_writes: s.denied_writes.clone(),
            }
        } else {
            Self::default_session_allowlist_only()
        }
    }

    /// Default: only `approval.session_allowlist` may be persisted when L2 has no self_adjust.
    pub fn default_session_allowlist_only() -> Self {
        Self {
            allowed_writes: vec![
                "approval.session_allowlist".into(),
                "approval.session_shell_binaries".into(),
                "approval.*".into(),
            ],
            denied_writes: vec![
                "security".into(),
                "security.*".into(),
                "gateway".into(),
                "gateway.*".into(),
                "channels".into(),
                "channels.*".into(),
            ],
        }
    }

    pub fn validate_write_path(&self, patch_path: &str) -> Result<()> {
        let path = patch_path.trim();
        if path.is_empty() {
            bail!("policy patch path must not be empty");
        }
        if self
            .denied_writes
            .iter()
            .any(|deny| path_matches(path, deny))
        {
            bail!("policy patch path denied by self_adjust: {path}");
        }
        if self.allowed_writes.is_empty() {
            bail!(
                "no self_adjust.allowed_writes configured; patch denied: {path}. \
                 Edit [autonomy] in config.toml, or add workspace agent-policy.yaml \
                 (see examples/profiles/agent-policy.self-adjust.yaml)."
            );
        }
        if self
            .allowed_writes
            .iter()
            .any(|allow| path_matches(path, allow))
        {
            return Ok(());
        }
        bail!(
            "policy patch path not in self_adjust.allowed_writes: {path}. \
             Without L2 self_adjust covering this path, edit config.toml directly \
             (seed: examples/profiles/agent-policy.self-adjust.yaml)."
        )
    }

    pub fn validate_session_allowlist_tool(&self, tool_name: &str) -> Result<()> {
        if tool_name.trim().is_empty() {
            bail!("tool name must not be empty");
        }
        self.validate_write_path("approval.session_allowlist")
    }

    pub fn validate_session_shell_binary(&self, binary: &str) -> Result<()> {
        if binary.trim().is_empty() {
            bail!("shell binary name must not be empty");
        }
        self.validate_write_path("approval.session_shell_binaries")
    }
}

/// Merge L2.5 overrides into resolved autonomy layer values.
pub fn merge_policy_overrides(
    mut base: crate::effective_execution_policy::AutonomyLayerValues,
    overrides: Option<&PolicyOverridesLayer>,
) -> crate::effective_execution_policy::AutonomyLayerValues {
    let Some(layer) = overrides else {
        return base;
    };

    if let Some(autonomy) = &layer.autonomy {
        if let Some(level) = &autonomy.level {
            base.level = level.clone();
        }
        if let Some(v) = autonomy.workspace_only {
            base.workspace_only = v;
        }
        if let Some(v) = &autonomy.allowed_commands {
            base.allowed_commands = v.clone();
        }
        if let Some(v) = &autonomy.forbidden_paths {
            base.forbidden_paths = v.clone();
        }
        if let Some(v) = autonomy.max_actions_per_hour {
            base.max_actions_per_hour = v;
        }
        if let Some(v) = autonomy.max_cost_per_day_cents {
            base.max_cost_per_day_cents = v;
        }
        if let Some(v) = autonomy.require_approval_for_medium_risk {
            base.require_approval_for_medium_risk = v;
        }
        if let Some(v) = autonomy.block_high_risk_commands {
            base.block_high_risk_commands = v;
        }
        if let Some(v) = &autonomy.auto_approve {
            base.auto_approve = v.clone();
        }
        if let Some(v) = &autonomy.always_ask {
            base.always_ask = v.clone();
        }
    }

    if let Some(approval) = &layer.approval {
        for tool in &approval.session_allowlist {
            if base.always_ask.iter().any(|t| t == tool) {
                continue;
            }
            if !base.auto_approve.iter().any(|t| t == tool) {
                base.auto_approve.push(tool.clone());
            }
        }
    }

    base
}

pub fn policy_overrides_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir
        .join(POLICY_OVERRIDES_DIR)
        .join(POLICY_OVERRIDES_FILE)
}

pub fn load_policy_overrides_from_path(path: &Path) -> Result<Option<PolicyOverridesLayer>> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    reject_forbidden_secret_keys(&raw)?;
    let layer: PolicyOverridesLayer =
        serde_yaml::from_str(&raw).context("parse policy-overrides.yaml")?;
    if let Some(version) = layer.version {
        if version != 1 {
            bail!("unsupported policy-overrides.yaml version: {version} (expected 1)");
        }
    }
    Ok(Some(layer))
}

pub fn discover_and_load_policy_overrides(
    workspace_dir: &Path,
) -> Result<Option<PolicyOverridesLayer>> {
    let path = policy_overrides_path(workspace_dir);
    load_policy_overrides_from_path(&path)
}

fn path_matches(path: &str, pattern: &str) -> bool {
    if pattern == path {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return path == prefix || path.starts_with(&format!("{prefix}."));
    }
    if pattern == "*" {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_allowlist_merges_into_auto_approve() {
        let base = crate::effective_execution_policy::AutonomyLayerValues {
            level: "supervised".into(),
            auto_approve: vec!["file_read".into()],
            always_ask: vec!["shell".into()],
            ..Default::default()
        };
        let overrides = PolicyOverridesLayer {
            version: Some(1),
            approval: Some(ApprovalOverridesSection {
                session_allowlist: vec!["file_write".into(), "shell".into()],
                ..Default::default()
            }),
            autonomy: None,
        };
        let merged = merge_policy_overrides(base, Some(&overrides));
        assert!(merged.auto_approve.contains(&"file_read".into()));
        assert!(merged.auto_approve.contains(&"file_write".into()));
        assert!(!merged.auto_approve.contains(&"shell".into()));
    }

    #[test]
    fn self_adjust_denies_security_writes() {
        let enforcer = SelfAdjustEnforcer::default_session_allowlist_only();
        assert!(enforcer
            .validate_write_path("approval.session_allowlist")
            .is_ok());
        assert!(enforcer
            .validate_write_path("security.audit.enabled")
            .is_err());
    }

    #[test]
    fn rejects_secret_keys_in_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy-overrides.yaml");
        std::fs::write(
            &path,
            "approval:\n  session_allowlist:\n    - shell\napi_key: x\n",
        )
        .unwrap();
        assert!(load_policy_overrides_from_path(&path).is_err());
    }
}
