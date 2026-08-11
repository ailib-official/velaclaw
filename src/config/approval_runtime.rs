//! Approval manager wiring — persistence + security audit (VL-SEC-004).
//! 将 L2.5 持久化与 `security.audit` 桥接到 [`ApprovalManager`]。

use super::policy_overrides::PolicyOverridesStore;
use super::{discover_and_load, resolve_effective_autonomy, AutonomyConfig, Config};
use crate::approval::ApprovalManager;
use crate::security::AuditLogger;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;

/// Shared wiring for constructing [`ApprovalManager`] instances with L2.5 + audit.
#[derive(Clone)]
pub struct ApprovalManagerWiring {
    overrides_store: Arc<PolicyOverridesStore>,
    security_audit: Option<Arc<AuditLogger>>,
}

impl ApprovalManagerWiring {
    pub fn from_config(config: &Config) -> Result<Self> {
        let l2 = discover_and_load(config)?;
        Ok(Self {
            overrides_store: Arc::new(PolicyOverridesStore::new(config, l2.as_ref())),
            security_audit: Self::audit_logger_from_config(config)?,
        })
    }

    pub fn spawn_manager(&self, autonomy: &AutonomyConfig) -> ApprovalManager {
        let mut mgr = ApprovalManager::from_config(autonomy);
        mgr = mgr.with_overrides_store(Arc::clone(&self.overrides_store));
        if let Some(audit) = &self.security_audit {
            mgr = mgr.with_security_audit(Arc::clone(audit));
        }
        if let Ok(Some(layer)) = self.overrides_store.load() {
            if let Some(approval) = layer.approval {
                mgr.seed_session_shell_binaries(approval.session_shell_binaries);
            }
        }
        mgr
    }

    fn audit_logger_from_config(config: &Config) -> Result<Option<Arc<AuditLogger>>> {
        if !config.security.audit.enabled {
            return Ok(None);
        }
        let velaclaw_dir = config
            .config_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| config.workspace_dir.clone());
        let logger = AuditLogger::new(config.security.audit.clone(), velaclaw_dir)
            .context("create security audit logger")?;
        Ok(Some(Arc::new(logger)))
    }
}

/// Resolve effective autonomy and build a fully wired [`ApprovalManager`].
pub fn create_approval_manager(config: &Config) -> Result<ApprovalManager> {
    let autonomy = resolve_effective_autonomy(config)?;
    let wiring = ApprovalManagerWiring::from_config(config)?;
    Ok(wiring.spawn_manager(&autonomy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::ApprovalResponse;
    use tempfile::TempDir;

    #[test]
    fn record_decision_writes_security_audit_when_enabled() -> Result<()> {
        let dir = TempDir::new()?;
        let mut config = Config::default();
        config.workspace_dir = dir.path().join("workspace");
        config.config_path = dir.path().join("config.toml");
        config.security.audit.enabled = true;

        let mgr = create_approval_manager(&config)?;
        mgr.record_decision(
            "file_write",
            &serde_json::json!({"path": "out.txt"}),
            ApprovalResponse::Yes,
            "cli",
        );

        let log_path = dir.path().join("audit.log");
        assert!(log_path.is_file(), "security audit log must be created");
        let content = std::fs::read_to_string(log_path)?;
        assert!(content.contains("tool_approval"));
        assert!(content.contains("file_write"));
        Ok(())
    }
}
