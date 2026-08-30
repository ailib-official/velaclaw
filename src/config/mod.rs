#[cfg(feature = "ai-protocol")]
pub mod agent_policy;
#[cfg(feature = "ai-protocol")]
pub mod approval_runtime;
#[cfg(feature = "ai-protocol")]
pub mod effective_execution_policy;
#[cfg(feature = "ai-protocol")]
pub mod effective_policy;
#[cfg(feature = "ai-protocol")]
pub mod policy_overrides;
pub mod runtime_bootstrap;
pub mod schema;

#[cfg(feature = "ai-protocol")]
pub use agent_policy::{discover_and_load, AgentPolicyLayer};

pub use runtime_bootstrap::{bootstrap_runtime, BootstrapOptions, RuntimeBootstrap};
pub use velaclaw_config::defaults::{DEFAULT_PROTOCOL_MODEL_ID, DEFAULT_PROTOCOL_MODEL_LABEL};

#[allow(unused_imports)]
pub use schema::{
    apply_runtime_proxy_to_builder, build_runtime_proxy_client,
    build_runtime_proxy_client_with_timeouts, runtime_proxy_config, set_runtime_proxy_config,
    AgentConfig, AuditConfig, AutonomyConfig, BrowserComputerUseConfig, BrowserConfig,
    ChannelApprovalMode, ChannelsConfig, ClassificationRule, CliRenderConfig, ComposioConfig,
    Config, CostConfig, CronConfig, DelegateAgentConfig, DeployConfig, DeploymentSettingsConfig,
    DeploymentTargetConfig, DiscordConfig, DockerRuntimeConfig, EmbeddingRouteConfig,
    ExecutionRoutingConfig, GatewayConfig, HardwareConfig, HardwareTransport, HeartbeatConfig,
    HttpRequestConfig, IMessageConfig, IdentityConfig, LarkConfig, MatrixConfig, MemoryConfig,
    ModelRouteConfig, ModelRoutePeerConfig, MultimodalConfig, NextcloudTalkConfig,
    ObservabilityConfig, PeripheralBoardConfig, PeripheralsConfig, ProviderRoutingMode,
    ProxyConfig, ProxyScope, QueryClassificationConfig, ReliabilityConfig, ResourceLimitsConfig,
    RuntimeConfig, SandboxBackend, SandboxConfig, SchedulerConfig, SecretsConfig, SecurityConfig,
    SecurityProfile, SkillsConfig, SkillsPromptInjectionMode, SlackConfig, StorageConfig,
    StorageProviderConfig, StorageProviderSection, StreamMode, TelegramConfig, TunnelConfig,
    WasmRuntimeConfig, WebSearchConfig, WebhookConfig,
};

#[cfg(feature = "ai-protocol")]
pub use approval_runtime::{create_approval_manager, ApprovalManagerWiring};
#[cfg(feature = "ai-protocol")]
pub use effective_execution_policy::resolve_effective_autonomy;
#[cfg(feature = "ai-protocol")]
pub use effective_policy::{merge_tool_dispatcher, EffectivePolicy};
#[cfg(feature = "ai-protocol")]
pub use policy_overrides::{discover_policy_overrides, PolicyOverridesStore};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reexported_config_default_is_constructible() {
        let config = Config::default();

        assert!(config.default_provider.is_some());
        assert!(config.default_model.is_some());
        assert!(config.default_temperature > 0.0);
    }

    #[test]
    fn reexported_channel_configs_are_constructible() {
        let telegram = TelegramConfig {
            bot_token: "token".into(),
            allowed_users: vec!["alice".into()],
            stream_mode: StreamMode::default(),
            draft_update_interval_ms: 1000,
            interrupt_on_new_message: false,
            mention_only: false,
            approval_mode: ChannelApprovalMode::default(),
            approval_timeout_secs: 300,
        };

        let discord = DiscordConfig {
            bot_token: "token".into(),
            guild_id: Some("123".into()),
            allowed_users: vec![],
            listen_to_bots: false,
            mention_only: false,
        };

        let lark = LarkConfig {
            app_id: "app-id".into(),
            app_secret: "app-secret".into(),
            encrypt_key: None,
            verification_token: None,
            allowed_users: vec![],
            use_feishu: false,
            receive_mode: crate::config::schema::LarkReceiveMode::Websocket,
            port: None,
        };

        let nextcloud_talk = NextcloudTalkConfig {
            base_url: "https://cloud.example.com".into(),
            app_token: "app-token".into(),
            webhook_secret: None,
            allowed_users: vec!["*".into()],
        };

        assert_eq!(telegram.allowed_users.len(), 1);
        assert_eq!(discord.guild_id.as_deref(), Some("123"));
        assert_eq!(lark.app_id, "app-id");
        assert_eq!(nextcloud_talk.base_url, "https://cloud.example.com");
    }
}
