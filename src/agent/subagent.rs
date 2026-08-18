//! SubAgent dispatch/aggregate lifecycle (VL-MA-005).
//! 子 agent 分派/聚合：工具范围不得超出父 registry；禁止嵌套 delegate。

use std::collections::HashSet;

/// Host-side dispatch record before the child tool-loop runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgentDispatch {
    pub run_id: String,
    pub agent_name: String,
    pub parent_depth: u32,
    pub tool_names: Vec<String>,
}

/// Host-side aggregate record after the child tool-loop returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgentAggregate {
    pub run_id: String,
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl SubAgentDispatch {
    #[must_use]
    pub fn header_line(&self) -> String {
        format!(
            "[SubAgent dispatch run_id={} agent='{}' depth={} tools={}]",
            self.run_id,
            self.agent_name,
            self.parent_depth,
            self.tool_names.join(",")
        )
    }
}

impl SubAgentAggregate {
    #[must_use]
    pub fn footer_line(&self) -> String {
        format!(
            "[SubAgent aggregate run_id={} success={}]",
            self.run_id, self.success
        )
    }
}

/// Privilege failure when the child would exceed the parent tool set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgentPrivilegeError {
    pub message: String,
}

/// Intersect `allowlist` with parent tool names.
///
/// Fail-closed: nested `delegate` and names absent from the parent are errors,
/// not silently dropped (child cannot escalate).
pub fn resolve_subagent_scope(
    parent_names: &[&str],
    allowlist: &[String],
) -> Result<Vec<String>, SubAgentPrivilegeError> {
    let parent: HashSet<&str> = parent_names.iter().copied().collect();
    let mut scoped = Vec::new();
    let mut seen = HashSet::new();

    for raw in allowlist {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        if name == "delegate" {
            return Err(SubAgentPrivilegeError {
                message: "privilege: nested delegate is forbidden".into(),
            });
        }
        if !parent.contains(name) {
            return Err(SubAgentPrivilegeError {
                message: format!("privilege: tool '{name}' is not in the parent registry"),
            });
        }
        if seen.insert(name.to_string()) {
            scoped.push(name.to_string());
        }
    }

    if scoped.is_empty() {
        return Err(SubAgentPrivilegeError {
            message: "privilege: sub-agent has no executable tools after scoping".into(),
        });
    }

    Ok(scoped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_keeps_parent_intersection_in_allowlist_order() {
        let scoped = resolve_subagent_scope(
            &["file_read", "shell", "glob_search"],
            &[" glob_search ".into(), "shell".into(), "glob_search".into()],
        )
        .expect("scope");
        assert_eq!(scoped, vec!["glob_search", "shell"]);
    }

    #[test]
    fn scope_rejects_nested_delegate() {
        let err = resolve_subagent_scope(&["delegate", "file_read"], &["delegate".into()])
            .expect_err("nested");
        assert!(err.message.contains("nested delegate"));
    }

    #[test]
    fn scope_rejects_tools_not_on_parent() {
        let err = resolve_subagent_scope(&["file_read"], &["shell".into()]).expect_err("escalate");
        assert!(err.message.contains("not in the parent registry"));
    }

    #[test]
    fn dispatch_aggregate_share_run_id() {
        let dispatch = SubAgentDispatch {
            run_id: "subagent-test".into(),
            agent_name: "researcher".into(),
            parent_depth: 0,
            tool_names: vec!["file_read".into()],
        };
        let agg = SubAgentAggregate {
            run_id: dispatch.run_id.clone(),
            success: true,
            output: "ok".into(),
            error: None,
        };
        assert!(dispatch.header_line().contains("run_id=subagent-test"));
        assert!(agg.footer_line().contains("run_id=subagent-test"));
        assert!(agg.footer_line().contains("success=true"));
    }
}
