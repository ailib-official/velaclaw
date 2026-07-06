//! L2 workspace `agent-policy.yaml` loading and validation (VL-ARCH-006 / VL-ARCH-004).
//! 工作区策略文件：tool_calling、self_adjust 等；禁止 secret 字段。

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const AGENT_POLICY_FILE: &str = "agent-policy.yaml";
pub const AGENT_POLICY_WORKSPACE_SUBPATH: &str = "workspace/agent-policy.yaml";

/// Parsed subset of `agent-policy.yaml` (L2 workspace policy).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct AgentPolicyLayer {
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub tool_calling: Option<ToolCallingPolicySection>,
    #[serde(default)]
    pub self_adjust: Option<SelfAdjustSection>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ToolCallingPolicySection {
    pub dispatcher: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct SelfAdjustSection {
    #[serde(default)]
    pub allowed_writes: Vec<String>,
    #[serde(default)]
    pub denied_writes: Vec<String>,
}

impl AgentPolicyLayer {
    pub fn tool_dispatcher(&self) -> Option<&str> {
        self.tool_calling
            .as_ref()
            .and_then(|tc| tc.dispatcher.as_deref())
    }

    /// Discover and load L2 policy for the active workspace / project root.
    pub fn discover_and_load(workspace_dir: &Path) -> Result<Option<Self>> {
        let Some(path) = discover_agent_policy_path(workspace_dir)? else {
            return Ok(None);
        };
        let policy = Self::load_from_path(&path).with_context(|| {
            format!("failed to load workspace agent policy: {}", path.display())
        })?;
        Ok(Some(policy))
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        reject_forbidden_secret_keys(&raw)?;
        let policy: AgentPolicyLayer =
            serde_yaml::from_str(&raw).context("parse agent-policy.yaml")?;
        if let Some(version) = policy.version {
            if version != 1 {
                bail!("unsupported agent-policy.yaml version: {version} (expected 1)");
            }
        }
        Ok(policy)
    }
}

/// Walk cwd (and `VELACLAW_WORKSPACE` / workspace roots) for `agent-policy.yaml`.
pub fn discover_agent_policy_path(workspace_dir: &Path) -> Result<Option<PathBuf>> {
    let mut search_roots: Vec<PathBuf> = Vec::new();

    if let Ok(custom) = std::env::var("VELACLAW_WORKSPACE") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            search_roots.push(PathBuf::from(trimmed));
        }
    }

    if !workspace_dir.as_os_str().is_empty() {
        if let Some(parent) = workspace_dir.parent() {
            if parent != workspace_dir {
                search_roots.push(parent.to_path_buf());
            }
        }
        search_roots.push(workspace_dir.to_path_buf());
    }

    if let Ok(cwd) = std::env::current_dir() {
        search_roots.push(cwd);
    }

    let mut seen = std::collections::HashSet::new();
    for root in search_roots {
        let mut current = Some(root.as_path());
        while let Some(dir) = current {
            if !seen.insert(dir.to_path_buf()) {
                current = dir.parent();
                continue;
            }
            if let Some(found) = policy_file_in_dir(dir) {
                return Ok(Some(found));
            }
            current = dir.parent();
        }
    }

    Ok(None)
}

fn policy_file_in_dir(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join(AGENT_POLICY_FILE);
    if direct.is_file() {
        return Some(direct);
    }
    let nested = dir.join(AGENT_POLICY_WORKSPACE_SUBPATH);
    if nested.is_file() {
        return Some(nested);
    }
    None
}

const FORBIDDEN_YAML_KEYS: &[&str] = &[
    "api_key",
    "api-key",
    "secret",
    "token",
    "password",
    "credentials",
    "bot_token",
    "app_secret",
];

fn reject_forbidden_secret_keys(raw: &str) -> Result<()> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(raw).context("parse agent-policy.yaml for validation")?;
    let mut violations = Vec::new();
    collect_forbidden_keys(&value, "", &mut violations);
    if violations.is_empty() {
        return Ok(());
    }
    bail!(
        "agent-policy.yaml must not contain secret fields: {}",
        violations.join(", ")
    );
}

fn collect_forbidden_keys(value: &serde_yaml::Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (key, child) in map {
                let segment = mapping_key(key);
                let path = if prefix.is_empty() {
                    segment.clone()
                } else {
                    format!("{prefix}.{segment}")
                };
                if FORBIDDEN_YAML_KEYS
                    .iter()
                    .any(|forbidden| segment.eq_ignore_ascii_case(forbidden))
                {
                    out.push(path.clone());
                }
                collect_forbidden_keys(child, &path, out);
            }
        }
        serde_yaml::Value::Sequence(items) => {
            for (idx, item) in items.iter().enumerate() {
                let path = format!("{prefix}[{idx}]");
                collect_forbidden_keys(item, &path, out);
            }
        }
        _ => {}
    }
}

fn mapping_key(key: &serde_yaml::Value) -> String {
    match key {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_valid_agent_policy_yaml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(AGENT_POLICY_FILE);
        fs::write(
            &path,
            r#"
version: 1
tool_calling:
  dispatcher: xml
"#,
        )
        .unwrap();

        let policy = AgentPolicyLayer::load_from_path(&path).unwrap();
        assert_eq!(policy.tool_dispatcher(), Some("xml"));
    }

    #[test]
    fn reject_secret_fields_in_yaml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(AGENT_POLICY_FILE);
        fs::write(
            &path,
            r#"
channels:
  telegram:
    bot_token: "secret"
"#,
        )
        .unwrap();

        let err = AgentPolicyLayer::load_from_path(&path).unwrap_err();
        assert!(err.to_string().contains("secret fields"));
    }

    #[test]
    fn discover_from_cwd_walk_up() {
        let root = TempDir::new().unwrap();
        let nested = root.path().join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            root.path().join(AGENT_POLICY_FILE),
            "version: 1\ntool_calling:\n  dispatcher: native\n",
        )
        .unwrap();

        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&nested).unwrap();
        let found = discover_agent_policy_path(root.path()).unwrap();
        std::env::set_current_dir(prev).unwrap();

        assert_eq!(found, Some(root.path().join(AGENT_POLICY_FILE)));
    }
}
