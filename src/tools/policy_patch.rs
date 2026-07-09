use super::traits::{Tool, ToolExecutionContext, ToolResult};
use crate::config::Config;
use crate::config::PolicyOverridesStore;
use crate::security::policy::ToolOperation;
use crate::security::PolicyHandle;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Apply validated dot-path patches to L2.5 `policy-overrides.yaml` (VL-SEC-005).
pub struct PolicyPatchTool {
    config: Arc<Config>,
    store: Arc<PolicyOverridesStore>,
    security: PolicyHandle,
}

impl PolicyPatchTool {
    pub fn new(
        config: Arc<Config>,
        store: Arc<PolicyOverridesStore>,
        security: PolicyHandle,
    ) -> Self {
        Self {
            config,
            store,
            security,
        }
    }
}

#[async_trait]
impl Tool for PolicyPatchTool {
    fn name(&self) -> &str {
        "policy_patch"
    }

    fn description(&self) -> &str {
        "Apply a dot-path patch to workspace policy-overrides.yaml (L2.5). \
         Paths must match self_adjust.allowed_writes in agent-policy.yaml; \
         security.* and channels.* are denied by default. \
         Autonomy patches take effect immediately; approval list patches persist for the next session reload."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Dot path under policy-overrides.yaml (e.g. autonomy.allowed_commands, approval.session_allowlist)"
                },
                "value": {
                    "description": "JSON value to set at the path (string, bool, number, or array)"
                }
            },
            "required": ["path", "value"]
        })
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?
            .trim();
        let value = args
            .get("value")
            .ok_or_else(|| anyhow::anyhow!("Missing 'value' parameter"))?;

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "policy_patch")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: rate limit exceeded".into()),
            });
        }

        match self.store.apply_patch(path, value.clone()) {
            Ok(saved_path) => {
                if path.starts_with("autonomy.") {
                    if let Err(err) = self.security.refresh_from_config(&self.config) {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Patch saved to {} but in-memory policy refresh failed: {err}",
                                saved_path.display()
                            )),
                        });
                    }
                }

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Policy patch applied at `{path}` → saved to {}.",
                        saved_path.display()
                    ),
                    error: None,
                })
            }
            Err(err) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Policy patch rejected: {err}")),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::discover_and_load;
    use crate::security::{AutonomyLevel, PolicyHandle, SecurityPolicy};
    use std::fs;
    use tempfile::TempDir;
    use velaclaw_config::agent_policy::AGENT_POLICY_FILE;

    fn supervised_policy() -> PolicyHandle {
        PolicyHandle::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            ..SecurityPolicy::default()
        })
    }

    #[tokio::test]
    async fn policy_patch_blocks_readonly_mode() {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        let store = Arc::new(PolicyOverridesStore::new(&config, None));
        let readonly = PolicyHandle::new(SecurityPolicy {
            autonomy: AutonomyLevel::ReadOnly,
            ..SecurityPolicy::default()
        });
        let tool = PolicyPatchTool::new(Arc::new(config), store, readonly);

        let result = tool
            .execute(
                json!({"path": "autonomy.level", "value": "full"}),
                &ToolExecutionContext::default(),
            )
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("read-only"));
    }

    #[tokio::test]
    async fn policy_patch_denies_security_path() {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        let store = Arc::new(PolicyOverridesStore::new(&config, None));
        let tool = PolicyPatchTool::new(Arc::new(config), store, supervised_policy());

        let result = tool
            .execute(
                json!({"path": "security.audit.enabled", "value": true}),
                &ToolExecutionContext::default(),
            )
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("policy patch path denied"));
    }

    #[tokio::test]
    async fn policy_patch_allows_configured_memory_preferences_path() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(AGENT_POLICY_FILE),
            r#"
version: 2
self_adjust:
  allowed_writes:
    - memory.preferences.*
  denied_writes:
    - security.*
"#,
        )
        .unwrap();

        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        let l2 = discover_and_load(&config).unwrap();
        let store = Arc::new(PolicyOverridesStore::new(&config, l2.as_ref()));
        let tool = PolicyPatchTool::new(Arc::new(config), store, supervised_policy());

        let result = tool
            .execute(
                json!({"path": "memory.preferences.tone", "value": "formal"}),
                &ToolExecutionContext::default(),
            )
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result
            .error
            .unwrap()
            .contains("unsupported policy patch path"));
    }

    #[tokio::test]
    async fn policy_patch_autonomy_refreshes_in_memory_policy() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(AGENT_POLICY_FILE),
            r#"
version: 2
self_adjust:
  allowed_writes:
    - autonomy.*
"#,
        )
        .unwrap();

        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        config.autonomy.allowed_commands = vec!["ls".into()];
        let l2 = discover_and_load(&config).unwrap();
        let store = Arc::new(PolicyOverridesStore::new(&config, l2.as_ref()));
        let security = PolicyHandle::from_workspace_config(&config).unwrap();
        let tool = PolicyPatchTool::new(Arc::new(config.clone()), store, security.clone());

        let result = tool
            .execute(
                json!({"path": "autonomy.allowed_commands", "value": ["echo"]}),
                &ToolExecutionContext::default(),
            )
            .await
            .unwrap();

        assert!(result.success, "{}", result.error.unwrap_or_default());
        assert_eq!(security.read().allowed_commands, vec!["echo".to_string()]);
    }
}
