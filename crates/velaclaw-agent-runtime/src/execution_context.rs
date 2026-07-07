//! Per-invocation tool execution context (VL-UR-001).
//! 单次工具调用上下文：模型不可见的能力位（如人类 shell 批准）。

use serde::{Deserialize, Serialize};

/// Runtime-injected capabilities for one tool invocation.
///
/// Values are set by the agent loop / approval gate — never from LLM tool JSON.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExecutionContext {
    /// Human confirmed medium/high-risk shell policy for this invocation.
    pub human_shell_approved: bool,
}

impl ToolExecutionContext {
    pub fn with_shell_human_approved(approved: bool) -> Self {
        Self {
            human_shell_approved: approved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_denies_shell_approval() {
        assert!(!ToolExecutionContext::default().human_shell_approved);
    }

    #[test]
    fn with_shell_human_approved_sets_flag() {
        let ctx = ToolExecutionContext::with_shell_human_approved(true);
        assert!(ctx.human_shell_approved);
    }
}
