//! Host hop stop classes (VL-NA-043). One table for probe + DAG boundary.
//! 单跳停机分类：策略拒绝 / 封顶 / 取消，禁止再编事后图。

/// How this hop should close after a shell batch (not DAG hop count).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HopClose {
    #[default]
    None,
    /// Four executed shells; remaining DAG nodes may still run.
    Cap,
    /// Same *terminal* policy class denied twice; store fail cursor, do not start later hops.
    PolicyDeny,
}

/// DAG / observe follow-up for [`HopClose`] (VL-NA-045 / P10 table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterHopClose {
    /// Store artifact; LLM observe may replan remaining.
    ObserveThenContinue,
    /// Store artifact; skip observe; walk the original remaining order.
    NextRemainingSkipObserve,
    /// `store_dag_fail`; do not start later hops; skip observe.
    FailCursorStop,
}

/// Host contract after a tool-loop hop. Cap must not fall through to observe.
#[must_use]
pub fn after_hop_close(close: HopClose) -> AfterHopClose {
    match close {
        HopClose::None => AfterHopClose::ObserveThenContinue,
        HopClose::Cap => AfterHopClose::NextRemainingSkipObserve,
        HopClose::PolicyDeny => AfterHopClose::FailCursorStop,
    }
}

impl AfterHopClose {
    #[must_use]
    pub fn skip_observe(self) -> bool {
        !matches!(self, AfterHopClose::ObserveThenContinue)
    }

    #[must_use]
    pub fn fail_cursor(self) -> bool {
        matches!(self, AfterHopClose::FailCursorStop)
    }
}

/// Operator-visible reason for a policy-deny hop stop (not internodal chrome).
#[must_use]
pub fn policy_deny_stop_reason(class: Option<&str>) -> &'static str {
    match class {
        Some("unsafe_construct") => {
            "repeated unsafe shell constructs (substitution, write-redirect, or blocked git flags)."
        }
        Some("allowlist") => "repeated commands not in the allowlist.",
        Some("malformed") => "malformed tool invocation.",
        Some("once_denied") => "a credential or privilege request was denied.",
        Some("wait") => "repeated wait-only commands.",
        _ => "repeated policy denials of the same class",
    }
}

/// Stable fail_class values for [`crate::agent::bounded_dag_live::DagFailCursor`].
pub const FAIL_CLASS_CANCELLED: &str = "cancelled";
pub const FAIL_CLASS_POLICY_DENY: &str = "policy_deny";

/// Resume the stored remaining chain without a repair-planner chat.
#[must_use]
pub fn keep_remaining_without_replan(fail_class: &str) -> bool {
    matches!(
        fail_class.trim(),
        FAIL_CLASS_CANCELLED | FAIL_CLASS_POLICY_DENY
    )
}

/// Policy-deny subclass so two unlike denials do not trip the hop stop.
#[must_use]
pub fn policy_deny_class(output: &str) -> Option<&'static str> {
    let t = output.to_ascii_lowercase();
    if t.contains("[needs_approval]") || t.contains("approve once") {
        return None;
    }
    if t.contains("[once_denied]") {
        return Some("once_denied");
    }
    if t.contains("malformed invocation") {
        return Some("malformed");
    }
    if t.contains("unsafe shell construct") {
        return Some("unsafe_construct");
    }
    if t.contains("not in allowed_commands") || t.contains("not allowed by security policy") {
        return Some("allowlist");
    }
    if t.contains("wait-only") {
        return Some("wait");
    }
    if t.contains("[policy_deny]")
        || (t.contains("denied") && (t.contains("policy") || t.contains("security")))
    {
        return Some("other_policy");
    }
    None
}

/// Classes that close the hop on the first deny (not two unlike buckets).
#[must_use]
pub fn policy_deny_closes_on_first(class: &str) -> bool {
    matches!(class, "malformed" | "once_denied")
}

/// Allowlist / wait-only misses are recoverable (drop the named binary; do not fail the DAG).
/// Cap the hop after this many so a deny loop cannot run forever.
pub const MAX_RECOVERABLE_POLICY_DENIES_BEFORE_CAP: u32 = 4;

#[must_use]
pub fn policy_deny_is_recoverable(class: &str) -> bool {
    matches!(class, "allowlist" | "wait")
}

/// Merge hop-close outcomes; PolicyDeny wins over Cap.
#[must_use]
pub fn merge_hop_close(current: HopClose, proposed: HopClose) -> HopClose {
    match (current, proposed) {
        (HopClose::PolicyDeny, _) | (_, HopClose::PolicyDeny) => HopClose::PolicyDeny,
        (HopClose::Cap, _) | (_, HopClose::Cap) => HopClose::Cap,
        _ => HopClose::None,
    }
}

/// Hop close implied by a running tally of one policy-deny class.
#[must_use]
pub fn hop_close_after_policy_tally(class: &str, n: u32) -> HopClose {
    if n == 0 {
        return HopClose::None;
    }
    if policy_deny_closes_on_first(class) {
        return HopClose::PolicyDeny;
    }
    if policy_deny_is_recoverable(class) {
        if n >= MAX_RECOVERABLE_POLICY_DENIES_BEFORE_CAP {
            return HopClose::Cap;
        }
        return HopClose::None;
    }
    if n >= 2 {
        return HopClose::PolicyDeny;
    }
    HopClose::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_and_policy_keep_remaining() {
        assert!(keep_remaining_without_replan(FAIL_CLASS_CANCELLED));
        assert!(keep_remaining_without_replan(FAIL_CLASS_POLICY_DENY));
        assert!(!keep_remaining_without_replan("unavailable"));
        assert!(!keep_remaining_without_replan(""));
    }

    #[test]
    fn once_prompt_is_not_policy_deny_class() {
        assert!(policy_deny_class("[needs_approval] approve Once").is_none());
    }

    #[test]
    fn malformed_and_once_denied_are_policy_classes() {
        assert_eq!(
            policy_deny_class("[policy_deny] malformed invocation: tool-call carrier in command."),
            Some("malformed")
        );
        assert_eq!(
            policy_deny_class("[once_denied] Denied by user after shell-policy approval."),
            Some("once_denied")
        );
        assert!(policy_deny_closes_on_first("malformed"));
        assert!(policy_deny_closes_on_first("once_denied"));
        assert!(!policy_deny_closes_on_first("allowlist"));
        assert_eq!(policy_deny_class("Denied by user."), None);
        assert_eq!(hop_close_after_policy_tally("allowlist", 2), HopClose::None);
        assert_eq!(hop_close_after_policy_tally("wait", 2), HopClose::None);
        assert_eq!(
            hop_close_after_policy_tally("allowlist", MAX_RECOVERABLE_POLICY_DENIES_BEFORE_CAP),
            HopClose::Cap
        );
        assert_eq!(
            hop_close_after_policy_tally("wait", MAX_RECOVERABLE_POLICY_DENIES_BEFORE_CAP),
            HopClose::Cap
        );
        assert_eq!(
            hop_close_after_policy_tally("unsafe_construct", 2),
            HopClose::PolicyDeny
        );
        assert_eq!(
            merge_hop_close(HopClose::Cap, HopClose::PolicyDeny),
            HopClose::PolicyDeny
        );
    }

    #[test]
    fn hop_cap_skips_observe_and_keeps_remaining_dag() {
        assert_eq!(
            after_hop_close(HopClose::Cap),
            AfterHopClose::NextRemainingSkipObserve
        );
        assert_eq!(
            after_hop_close(HopClose::PolicyDeny),
            AfterHopClose::FailCursorStop
        );
        assert_eq!(
            after_hop_close(HopClose::None),
            AfterHopClose::ObserveThenContinue
        );
        assert!(after_hop_close(HopClose::Cap).skip_observe());
        assert!(after_hop_close(HopClose::PolicyDeny).fail_cursor());
        assert!(policy_deny_stop_reason(Some("unsafe_construct")).contains("unsafe"));
        assert!(policy_deny_stop_reason(None).contains("same class"));
    }
}
