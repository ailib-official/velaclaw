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

/// Explicit Plan classification for known registry names (VL-MA-005 / #257).
/// `true` = mutating (blocked in Plan). Names must match live `Tool::name()`.
pub(crate) const REGISTRY_PLAN_CLASS: &[(&str, bool)] = &[
    ("request_human_input", false),
    ("shell", true),
    ("file_read", false),
    ("file_write", true),
    ("glob_search", false),
    ("cron_add", true),
    ("cron_list", false),
    ("cron_remove", true),
    ("cron_update", true),
    ("cron_run", true),
    ("cron_runs", false),
    ("memory_store", true),
    ("memory_recall", false),
    ("memory_forget", true),
    ("schedule", true),
    ("proxy_config", true),
    ("git_operations", true),
    ("pushover", true),
    ("browser_open", true),
    ("browser", true),
    ("http_request", true),
    ("web_search_tool", false),
    ("pdf_read", false),
    ("screenshot", true),
    ("image_info", false),
    ("composio", true),
    ("delegate", true),
    ("policy_patch", true),
    ("generative_capability", true),
    ("hardware_board_info", false),
    ("hardware_memory_map", false),
    ("hardware_memory_read", false),
    ("wasm_invoke", true),
];

pub(crate) fn pinned_plan_mutating(name: &str) -> Option<bool> {
    REGISTRY_PLAN_CLASS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, mutating)| *mutating)
}

pub(crate) fn is_mutating_tool(name: &str) -> bool {
    pinned_plan_mutating(name).unwrap_or_else(|| {
        // Legacy alias from #257; live tool name is `generative_capability`.
        name == "generative"
    })
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

    #[test]
    fn pin_table_agrees_with_is_mutating_tool() {
        for (name, mutating) in REGISTRY_PLAN_CLASS {
            assert_eq!(is_mutating_tool(name), *mutating, "pin mismatch for {name}");
        }
        assert!(is_mutating_tool("generative_capability"));
        assert!(is_mutating_tool("generative"));
        assert!(is_mutating_tool("wasm_invoke"));
        assert!(!is_mutating_tool("web_search_tool"));
    }
}
