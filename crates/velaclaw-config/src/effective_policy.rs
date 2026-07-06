//! EffectivePolicy — layered config merge (VL-ARCH-003/005); runtime dispatcher build stays in velaclaw.
//! 运行时策略快照：合并 L1/L2/session；`build_dispatcher` 在 velaclaw 主 crate。

use ai_lib_rust::ToolCallingPolicy;

/// Resolved tool-calling policy for one agent session / model binding.
#[derive(Debug, Clone)]
pub struct EffectivePolicy {
    /// L1 `agent.tool_dispatcher` or session override (`auto` | `native` | `xml`).
    pub tool_dispatcher: String,
    /// L0 manifest-derived parser + native strategy.
    pub tool_calling: ToolCallingPolicy,
}

impl EffectivePolicy {
    /// Merge L2 workspace → L1 config → session override over manifest policy (L0).
    pub fn resolve(
        config_tool_dispatcher: &str,
        workspace_tool_dispatcher: Option<&str>,
        session_tool_dispatcher: Option<&str>,
        tool_calling: ToolCallingPolicy,
    ) -> Self {
        let tool_dispatcher = merge_tool_dispatcher(
            config_tool_dispatcher,
            workspace_tool_dispatcher,
            session_tool_dispatcher,
        );
        Self {
            tool_dispatcher,
            tool_calling,
        }
    }
}

/// Priority: session > L1 config > L2 workspace > `auto`.
pub fn merge_tool_dispatcher(
    config_tool_dispatcher: &str,
    workspace_tool_dispatcher: Option<&str>,
    session_tool_dispatcher: Option<&str>,
) -> String {
    if let Some(session) = session_tool_dispatcher {
        return session.to_string();
    }
    if !config_tool_dispatcher.is_empty() {
        return config_tool_dispatcher.to_string();
    }
    workspace_tool_dispatcher
        .map(str::to_string)
        .unwrap_or_else(|| "auto".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_lib_rust::types::{
        text_tool::{PromptLevel, TextToolConfig},
        StandardTextToolParser,
    };
    use ai_lib_rust::{NativeStrategy, ToolCallingPolicy};

    fn sample_policy() -> ToolCallingPolicy {
        ToolCallingPolicy {
            parser: StandardTextToolParser::new(TextToolConfig {
                lenient_parsing: true,
                prompt_level: PromptLevel::L2,
                ..Default::default()
            }),
            native_strategy: NativeStrategy::Hybrid,
        }
    }

    #[test]
    fn merge_l1_overrides_l2() {
        assert_eq!(merge_tool_dispatcher("native", Some("xml"), None), "native");
    }

    #[test]
    fn resolve_l1_overrides_l2_workspace_baseline() {
        let policy = EffectivePolicy::resolve("native", Some("xml"), None, sample_policy());
        assert_eq!(policy.tool_dispatcher, "native");
    }
}
