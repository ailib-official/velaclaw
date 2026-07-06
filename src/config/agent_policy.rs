//! L2 agent-policy shim — delegates to `velaclaw-config` (VL-ARCH-004).

pub use velaclaw_config::agent_policy::*;

use super::Config;
use anyhow::Result;

/// Load L2 policy using workspace hints from a loaded [`Config`].
pub fn discover_and_load(config: &Config) -> Result<Option<AgentPolicyLayer>> {
    AgentPolicyLayer::discover_and_load(&config.workspace_dir)
}
