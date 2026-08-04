//! Per-invocation tool execution context (VL-UR-001).
//! 单次工具调用上下文：模型不可见的能力位（如人类 shell 批准）。

use serde::{Deserialize, Serialize};

/// Runtime-injected capabilities for one tool invocation.
///
/// Values are set by the agent loop / approval gate — never from LLM tool JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExecutionContext {
    /// Human confirmed medium/high-risk shell policy for this invocation.
    pub human_shell_approved: bool,
    /// Optional secret piped to the process stdin (e.g. `sudo -S`).
    /// Never sourced from model JSON; agent resolves opaque `secret_slot` ids.
    #[serde(skip)]
    pub stdin_secret: Option<String>,
}

impl ToolExecutionContext {
    pub fn with_shell_human_approved(approved: bool) -> Self {
        Self {
            human_shell_approved: approved,
            stdin_secret: None,
        }
    }

    pub fn with_stdin_secret(mut self, secret: Option<String>) -> Self {
        self.stdin_secret = secret;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_denies_shell_approval() {
        assert!(!ToolExecutionContext::default().human_shell_approved);
        assert!(ToolExecutionContext::default().stdin_secret.is_none());
    }

    #[test]
    fn with_shell_human_approved_sets_flag() {
        let ctx = ToolExecutionContext::with_shell_human_approved(true);
        assert!(ctx.human_shell_approved);
    }

    #[test]
    fn with_stdin_secret_sets_value() {
        let ctx = ToolExecutionContext::with_shell_human_approved(false)
            .with_stdin_secret(Some("pw".into()));
        assert_eq!(ctx.stdin_secret.as_deref(), Some("pw"));
    }
}
