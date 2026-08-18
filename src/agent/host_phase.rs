//! Host Plan vs Build policy (VL-MA-004). Planning stays on the host; not a toolbox E tool.
//! 宿主 Plan/Build 策略：规划不进 toolbox E。

/// Per-turn host execution phase. Default Build preserves current tool-loop behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostPhase {
    Plan,
    #[default]
    Build,
}

impl HostPhase {
    pub fn parse_opt(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("plan") => Self::Plan,
            _ => Self::Build,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
        }
    }

    /// Mutating tools are blocked in Plan so a rejected plan cannot change the workspace.
    pub fn blocks_mutating_tool(self, tool_name: &str) -> bool {
        self == Self::Plan && is_mutating_tool(tool_name)
    }

    pub fn blocked_output(self, tool_name: &str) -> Option<String> {
        if self.blocks_mutating_tool(tool_name) {
            Some(format!(
                "Plan phase: mutating tool '{tool_name}' was not executed. \
                 Approve Build to run mutating tools; remaining in Plan leaves the workspace unchanged."
            ))
        } else {
            None
        }
    }

    pub fn system_note(self) -> Option<&'static str> {
        match self {
            Self::Plan => Some(
                "Host phase is Plan: you may read and recall, but mutating tools \
                 (shell, file_write, git_operations, browser, cron write, memory_store, delegate, …) \
                 are blocked until the operator switches to Build.",
            ),
            Self::Build => None,
        }
    }
}

fn is_mutating_tool(name: &str) -> bool {
    matches!(
        name,
        "shell"
            | "file_write"
            | "git_operations"
            | "browser"
            | "browser_open"
            | "cron_add"
            | "cron_remove"
            | "cron_run"
            | "cron_update"
            | "memory_store"
            | "memory_forget"
            | "delegate"
            | "http_request"
            | "composio"
            | "policy_patch"
            | "proxy_config"
            | "schedule"
            | "pushover"
            | "screenshot"
            | "generative"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_allows_shell() {
        assert!(!HostPhase::Build.blocks_mutating_tool("shell"));
        assert!(HostPhase::Build.blocked_output("shell").is_none());
    }

    #[test]
    fn plan_blocks_shell_not_file_read() {
        assert!(HostPhase::Plan.blocks_mutating_tool("shell"));
        assert!(HostPhase::Plan.blocks_mutating_tool("file_write"));
        assert!(!HostPhase::Plan.blocks_mutating_tool("file_read"));
        assert!(!HostPhase::Plan.blocks_mutating_tool("memory_recall"));
        let blocked = HostPhase::Plan.blocked_output("shell").expect("blocked");
        assert!(blocked.contains("Plan phase"));
    }

    #[test]
    fn parse_defaults_to_build() {
        assert_eq!(HostPhase::parse_opt(None), HostPhase::Build);
        assert_eq!(HostPhase::parse_opt(Some("PLAN")), HostPhase::Plan);
        assert_eq!(HostPhase::parse_opt(Some("nope")), HostPhase::Build);
    }
}
