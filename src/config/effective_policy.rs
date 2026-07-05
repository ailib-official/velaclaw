//! EffectivePolicy — single merge point for layered configuration (VL-ARCH-003/005).
//! 运行时策略快照：合并 L1 config 与 L0 manifest（L2/L3 后续接入）。

#[cfg(feature = "ai-protocol")]
use crate::agent::dispatcher::{build_tool_dispatcher, ToolDispatcher};
#[cfg(feature = "ai-protocol")]
use crate::providers::Provider;
#[cfg(feature = "ai-protocol")]
use ai_lib_rust::ToolCallingPolicy;

/// Resolved tool-calling policy for one agent session / model binding.
#[cfg(feature = "ai-protocol")]
#[derive(Debug, Clone)]
pub struct EffectivePolicy {
    /// L1 `agent.tool_dispatcher` or session override (`auto` | `native` | `xml`).
    pub tool_dispatcher: String,
    /// L0 manifest-derived parser + native strategy.
    pub tool_calling: ToolCallingPolicy,
}

#[cfg(feature = "ai-protocol")]
impl EffectivePolicy {
    /// Merge config (L1) and optional session override over manifest policy (L0).
    pub fn resolve(
        config_tool_dispatcher: &str,
        session_tool_dispatcher: Option<&str>,
        tool_calling: ToolCallingPolicy,
    ) -> Self {
        let tool_dispatcher = session_tool_dispatcher
            .map(str::to_string)
            .unwrap_or_else(|| config_tool_dispatcher.to_string());
        Self {
            tool_dispatcher,
            tool_calling,
        }
    }

    /// Build the manifest-aware dispatcher for the resolved policy.
    pub fn build_dispatcher(&self, provider: &dyn Provider) -> Box<dyn ToolDispatcher> {
        build_tool_dispatcher(
            &self.tool_dispatcher,
            provider,
            self.tool_calling.clone(),
        )
    }
}

#[cfg(all(test, feature = "ai-protocol"))]
mod tests {
    use super::*;
    use ai_lib_rust::types::{
        text_tool::{PromptLevel, TextToolConfig},
        StandardTextToolParser,
    };
    use ai_lib_rust::{NativeStrategy, ToolCallingPolicy};
    use async_trait::async_trait;

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

    struct NativeCapableProvider;

    #[async_trait]
    impl Provider for NativeCapableProvider {
        fn supports_native_tools(&self) -> bool {
            true
        }

        async fn chat_with_system(
            &self,
            _system: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }

    struct NoNativeProvider;

    #[async_trait]
    impl Provider for NoNativeProvider {
        fn supports_native_tools(&self) -> bool {
            false
        }

        async fn chat_with_system(
            &self,
            _system: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }

    #[test]
    fn resolve_uses_config_when_no_session_override() {
        let policy = EffectivePolicy::resolve("auto", None, sample_policy());
        assert_eq!(policy.tool_dispatcher, "auto");
    }

    #[test]
    fn resolve_session_override_wins_over_config() {
        let policy = EffectivePolicy::resolve("auto", Some("xml"), sample_policy());
        assert_eq!(policy.tool_dispatcher, "xml");
    }

    #[test]
    fn build_dispatcher_auto_prefers_native_when_capable() {
        let effective = EffectivePolicy::resolve("auto", None, sample_policy());
        let dispatcher = effective.build_dispatcher(&NativeCapableProvider);
        assert!(dispatcher.should_send_tool_specs());
    }

    #[test]
    fn build_dispatcher_xml_never_sends_native_specs() {
        let effective = EffectivePolicy::resolve("xml", None, sample_policy());
        let dispatcher = effective.build_dispatcher(&NativeCapableProvider);
        assert!(!dispatcher.should_send_tool_specs());
    }

    #[test]
    fn build_dispatcher_auto_falls_back_to_xml_without_native() {
        let effective = EffectivePolicy::resolve("auto", None, sample_policy());
        let dispatcher = effective.build_dispatcher(&NoNativeProvider);
        assert!(!dispatcher.should_send_tool_specs());
    }
}
