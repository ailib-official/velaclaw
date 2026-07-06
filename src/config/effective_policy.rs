//! EffectivePolicy runtime wiring — `build_dispatcher` requires agent + Provider (VL-ARCH-004).
//! 主 crate 扩展：manifest-aware dispatcher 构建。

use crate::agent::dispatcher::{build_tool_dispatcher, ToolDispatcher};
use crate::providers::Provider;
pub use velaclaw_config::{merge_tool_dispatcher, EffectivePolicy as EffectivePolicyCore};

/// Resolved tool-calling policy with runtime dispatcher builder.
#[derive(Debug, Clone)]
pub struct EffectivePolicy(pub EffectivePolicyCore);

impl EffectivePolicy {
    pub fn resolve(
        config_tool_dispatcher: &str,
        workspace_tool_dispatcher: Option<&str>,
        session_tool_dispatcher: Option<&str>,
        tool_calling: ai_lib_rust::ToolCallingPolicy,
    ) -> Self {
        Self(EffectivePolicyCore::resolve(
            config_tool_dispatcher,
            workspace_tool_dispatcher,
            session_tool_dispatcher,
            tool_calling,
        ))
    }

    pub fn build_dispatcher(&self, provider: &dyn Provider) -> Box<dyn ToolDispatcher> {
        build_tool_dispatcher(
            &self.0.tool_dispatcher,
            provider,
            self.0.tool_calling.clone(),
        )
    }
}

impl std::ops::Deref for EffectivePolicy {
    type Target = EffectivePolicyCore;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
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
    fn build_dispatcher_auto_prefers_native_when_capable() {
        let effective = EffectivePolicy::resolve("auto", None, None, sample_policy());
        let dispatcher = effective.build_dispatcher(&NativeCapableProvider);
        assert!(dispatcher.should_send_tool_specs());
    }

    #[test]
    fn build_dispatcher_xml_never_sends_native_specs() {
        let effective = EffectivePolicy::resolve("xml", None, None, sample_policy());
        let dispatcher = effective.build_dispatcher(&NativeCapableProvider);
        assert!(!dispatcher.should_send_tool_specs());
    }

    #[test]
    fn build_dispatcher_auto_falls_back_to_xml_without_native() {
        let effective = EffectivePolicy::resolve("auto", None, None, sample_policy());
        let dispatcher = effective.build_dispatcher(&NoNativeProvider);
        assert!(!dispatcher.should_send_tool_specs());
    }
}
