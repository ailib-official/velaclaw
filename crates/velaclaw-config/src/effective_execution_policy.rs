//! Layered autonomy/approval merge for L1 config + L2 agent-policy (VL-SEC-001).
//! L1/L2 合并：工作区 agent-policy.yaml 覆盖 config.toml [autonomy] 子集。

use crate::agent_policy::{AgentPolicyLayer, ApprovalPolicySection, AutonomyPolicySection};

/// Portable autonomy + approval snapshot used across merge layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutonomyLayerValues {
    pub level: String,
    pub workspace_only: bool,
    pub allowed_commands: Vec<String>,
    pub forbidden_paths: Vec<String>,
    pub max_actions_per_hour: u32,
    pub max_cost_per_day_cents: u32,
    pub require_approval_for_medium_risk: bool,
    pub block_high_risk_commands: bool,
    pub auto_approve: Vec<String>,
    pub always_ask: Vec<String>,
}

/// Resolved execution policy after L1 + L2 merge (autonomy/approval subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveExecutionPolicy {
    pub autonomy: AutonomyLayerValues,
}

impl EffectiveExecutionPolicy {
    /// Merge L1 base values with optional L2 `agent-policy.yaml` overrides.
    pub fn resolve(l1: AutonomyLayerValues, l2: Option<&AgentPolicyLayer>) -> Self {
        let autonomy = match l2 {
            Some(layer) => {
                merge_autonomy_layers(&l1, layer.autonomy.as_ref(), layer.approval.as_ref())
            }
            None => l1,
        };
        Self { autonomy }
    }
}

/// Apply L2 overrides on top of L1. Present L2 fields replace L1; list fields replace wholesale.
pub fn merge_autonomy_layers(
    l1: &AutonomyLayerValues,
    l2_autonomy: Option<&AutonomyPolicySection>,
    l2_approval: Option<&ApprovalPolicySection>,
) -> AutonomyLayerValues {
    let mut out = l1.clone();

    if let Some(a) = l2_autonomy {
        if let Some(level) = &a.level {
            out.level = level.clone();
        }
        if let Some(v) = a.workspace_only {
            out.workspace_only = v;
        }
        if let Some(v) = &a.allowed_commands {
            out.allowed_commands = v.clone();
        }
        if let Some(v) = &a.forbidden_paths {
            out.forbidden_paths = v.clone();
        }
        if let Some(v) = a.max_actions_per_hour {
            out.max_actions_per_hour = v;
        }
        if let Some(v) = a.max_cost_per_day_cents {
            out.max_cost_per_day_cents = v;
        }
        if let Some(v) = a.require_approval_for_medium_risk {
            out.require_approval_for_medium_risk = v;
        }
        if let Some(v) = a.block_high_risk_commands {
            out.block_high_risk_commands = v;
        }
        if let Some(v) = &a.auto_approve {
            out.auto_approve = v.clone();
        }
        if let Some(v) = &a.always_ask {
            out.always_ask = v.clone();
        }
    }

    if let Some(ap) = l2_approval {
        if let Some(v) = &ap.auto_approve {
            out.auto_approve = v.clone();
        }
        if let Some(v) = &ap.always_ask {
            out.always_ask = v.clone();
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_policy::AgentPolicyLayer;

    fn sample_l1() -> AutonomyLayerValues {
        AutonomyLayerValues {
            level: "supervised".into(),
            workspace_only: true,
            allowed_commands: vec!["ls".into(), "cat".into()],
            forbidden_paths: vec!["/etc".into()],
            max_actions_per_hour: 20,
            max_cost_per_day_cents: 500,
            require_approval_for_medium_risk: true,
            block_high_risk_commands: true,
            auto_approve: vec!["file_read".into()],
            always_ask: vec![],
        }
    }

    #[test]
    fn l2_autonomy_overrides_l1_allowed_commands() {
        let l2 = AgentPolicyLayer {
            version: Some(2),
            autonomy: Some(AutonomyPolicySection {
                allowed_commands: Some(vec!["echo".into()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let resolved = EffectiveExecutionPolicy::resolve(sample_l1(), Some(&l2));
        assert_eq!(resolved.autonomy.allowed_commands, vec!["echo"]);
        assert_eq!(resolved.autonomy.level, "supervised");
    }

    #[test]
    fn l2_approval_overrides_auto_approve() {
        let l2 = AgentPolicyLayer {
            version: Some(2),
            approval: Some(ApprovalPolicySection {
                auto_approve: Some(vec!["shell".into()]),
                always_ask: Some(vec!["file_write".into()]),
            }),
            ..Default::default()
        };
        let resolved = EffectiveExecutionPolicy::resolve(sample_l1(), Some(&l2));
        assert_eq!(resolved.autonomy.auto_approve, vec!["shell"]);
        assert_eq!(resolved.autonomy.always_ask, vec!["file_write"]);
    }

    #[test]
    fn no_l2_passthrough_l1() {
        let l1 = sample_l1();
        let resolved = EffectiveExecutionPolicy::resolve(l1.clone(), None);
        assert_eq!(resolved.autonomy, l1);
    }
}
