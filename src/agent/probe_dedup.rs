//! Per-hop probe fingerprinting: skip repeat shells and script_vN ladders.
//! 同一 hop 内跳过重复探测与 script_vN 梯子。

use crate::agent::hop_stop::{policy_deny_class, HopClose};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const REPEAT_PROBE_NOTICE: &str = "Host skipped a repeat probe (same fingerprint as an earlier call this hop). Use INPUTS; compound remaining work or HANDOFF.";

pub const SHELL_ROUND_CAP_NOTICE: &str = "Host capped this hop at four executed shell rounds. Finish this node's internodal envelope from current INPUTS; do not start script_v2/v3.";

pub const MAX_SHELL_ROUNDS_PER_HOP: u32 = 4;

/// Probe skip/cap state for one DAG node (survives peer_continue and same-node re-entry).
#[derive(Debug, Default, Clone)]
pub struct HopProbeGovernor {
    seen: HashSet<String>,
    shell_rounds: u32,
    policy_denies: HashMap<&'static str, u32>,
    hop_close: HopClose,
    last_policy_class: Option<&'static str>,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeShellDecision {
    Run,
    SkipRepeat,
    Cap,
}

impl HopProbeGovernor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Count one assistant batch that actually ran a shell (not deny / skip / cap).
    pub fn record_executed_round(&mut self) {
        self.shell_rounds = self.shell_rounds.saturating_add(1);
        if self.shell_rounds >= MAX_SHELL_ROUNDS_PER_HOP {
            self.hop_close = HopClose::Cap;
        }
    }

    /// Drop a fingerprint that was reserved for Run but never executed (policy-deny / approval).
    pub fn retract_unexecuted(&mut self, fingerprint: &str) {
        self.seen.remove(fingerprint);
    }

    #[must_use]
    pub fn shell_rounds(&self) -> u32 {
        self.shell_rounds
    }

    #[must_use]
    pub fn decide_shell(&mut self, fingerprint: &str) -> ProbeShellDecision {
        if self.shell_rounds >= MAX_SHELL_ROUNDS_PER_HOP {
            self.notices.push(SHELL_ROUND_CAP_NOTICE.to_string());
            self.hop_close = HopClose::Cap;
            return ProbeShellDecision::Cap;
        }
        if self.seen.contains(fingerprint) {
            self.notices.push(REPEAT_PROBE_NOTICE.to_string());
            return ProbeShellDecision::SkipRepeat;
        }
        self.seen.insert(fingerprint.to_string());
        ProbeShellDecision::Run
    }

    pub fn drain_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    /// Classify a shell tool result: policy-deny tally and hop close.
    pub fn note_shell_output(&mut self, output: &str) {
        if output.contains(SHELL_ROUND_CAP_NOTICE) {
            self.hop_close = HopClose::Cap;
            return;
        }
        let Some(class) = policy_deny_class(output) else {
            return;
        };
        self.last_policy_class = Some(class);
        let n = self.policy_denies.entry(class).or_insert(0);
        *n = n.saturating_add(1);
        let proposed = crate::agent::hop_stop::hop_close_after_policy_tally(class, *n);
        self.hop_close = crate::agent::hop_stop::merge_hop_close(self.hop_close, proposed);
    }

    #[must_use]
    pub fn hop_close(&self) -> HopClose {
        self.hop_close
    }

    #[must_use]
    pub fn last_policy_deny_class(&self) -> Option<&'static str> {
        self.last_policy_class
    }
}

/// Cap / skip chrome is internodal (tool results), not the operator bubble.
#[must_use]
pub fn is_governor_chrome(text: &str) -> bool {
    let t = text.trim();
    t.contains(SHELL_ROUND_CAP_NOTICE) || t.contains(REPEAT_PROBE_NOTICE)
}

/// Notices that may be appended to the operator-visible prefix (VL-NA-045).
#[must_use]
pub fn operator_visible_notices(notices: Vec<String>) -> Vec<String> {
    notices
        .into_iter()
        .filter(|n| !is_governor_chrome(n))
        .collect()
}

/// User-visible hop body after tool-loop close. Must not substitute chrome.
#[must_use]
pub fn hop_close_visible_body(visible_text: &str, close: HopClose) -> String {
    if close == HopClose::None {
        return visible_text.to_string();
    }
    visible_text.trim().to_string()
}

/// True when this tool result consumed a host shell-round (VL-NA-041).
#[must_use]
pub fn shell_output_counts_as_round(output: &str) -> bool {
    let t = output.trim();
    if t.is_empty() {
        return false;
    }
    if t.contains(REPEAT_PROBE_NOTICE) || t.contains(SHELL_ROUND_CAP_NOTICE) {
        return false;
    }
    if t.contains("[policy_deny]") || t.contains("[sandbox_deny]") || t.contains("[needs_approval]")
    {
        return false;
    }
    let head = t.lines().next().unwrap_or(t);
    let l = head.to_ascii_lowercase();
    if l.contains("denied")
        && (l.contains("approval") || l.contains("policy") || l.contains("security"))
    {
        return false;
    }
    true
}

/// Fingerprint a tool call so equivalent probes collapse (whitespace + script version).
#[must_use]
pub fn tool_probe_fingerprint(name: &str, arguments: &Value) -> String {
    let n = name.trim().to_ascii_lowercase();
    if n == "shell" {
        let cmd = arguments
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("");
        format!("shell:{}", normalize_shell_command(cmd))
    } else {
        format!("{n}:{arguments}")
    }
}

#[must_use]
pub fn normalize_shell_command(command: &str) -> String {
    command
        .split_whitespace()
        .map(normalize_path_token)
        .collect::<Vec<_>>()
        .join(" ")
}

const SCRIPT_SUFFIXES: &[&str] = &[".py", ".sh", ".bash", ".js", ".ts"];

fn normalize_path_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    for suffix in SCRIPT_SUFFIXES {
        let Some(_) = lower.strip_suffix(suffix) else {
            continue;
        };
        let orig_stem_len = token.len().saturating_sub(suffix.len());
        let orig = &token[..orig_stem_len];
        let (dir, file) = match orig.rfind('/') {
            Some(i) => (&orig[..=i], &orig[i + 1..]),
            None => ("", orig),
        };
        return format!("{dir}{}{suffix}", strip_script_version(file));
    }
    token.to_string()
}

/// `xray_audit2` / `script_v3` / `confirm_v2` → unversioned stem.
#[must_use]
pub fn strip_script_version(stem: &str) -> String {
    if let Some(i) = stem.rfind("_v") {
        let rest = &stem[i + 2..];
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return stem[..i].to_string();
        }
    }
    let stripped = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    if stripped.len() >= 2
        && stripped.len() < stem.len()
        && stripped
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphabetic())
    {
        return stripped.to_string();
    }
    stem.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collapses_script_version_ladder() {
        let a = tool_probe_fingerprint(
            "shell",
            &json!({"command": "python3 /tmp/.velaclaw/xray_audit.py"}),
        );
        let b = tool_probe_fingerprint(
            "shell",
            &json!({"command": "python3 /tmp/.velaclaw/xray_audit2.py"}),
        );
        let c = tool_probe_fingerprint(
            "shell",
            &json!({"command": "python3 /tmp/.velaclaw/xray_audit_v3.py"}),
        );
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn collapses_whitespace() {
        let a = tool_probe_fingerprint("shell", &json!({"command": "arp -an"}));
        let b = tool_probe_fingerprint("shell", &json!({"command": "  arp   -an  "}));
        assert_eq!(a, b);
    }

    #[test]
    fn leaves_python3_token_alone() {
        assert_eq!(normalize_path_token("python3"), "python3");
    }

    #[test]
    fn repeat_set() {
        let mut seen = HashSet::new();
        let fp = tool_probe_fingerprint("shell", &json!({"command": "pwd"}));
        assert!(!seen.contains(&fp));
        seen.insert(fp.clone());
        assert!(seen.contains(&fp));
    }

    #[test]
    fn collapses_shell_script_version_ladder() {
        let a =
            tool_probe_fingerprint("shell", &json!({"command": "bash /tmp/.velaclaw/final.sh"}));
        let b = tool_probe_fingerprint(
            "shell",
            &json!({"command": "bash /tmp/.velaclaw/final2.sh"}),
        );
        let c = tool_probe_fingerprint(
            "shell",
            &json!({"command": "bash /tmp/.velaclaw/final_v3.sh"}),
        );
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn governor_survives_reentry_and_caps_fifth_round() {
        let mut g = HopProbeGovernor::new();
        for round in 1..=4 {
            let fp = tool_probe_fingerprint("shell", &json!({"command": format!("echo {round}")}));
            assert_eq!(g.decide_shell(&fp), ProbeShellDecision::Run);
            g.record_executed_round();
        }
        let fp = tool_probe_fingerprint("shell", &json!({"command": "echo 5"}));
        assert_eq!(g.decide_shell(&fp), ProbeShellDecision::Cap);
        let fp2 = tool_probe_fingerprint("shell", &json!({"command": "echo 6"}));
        assert_eq!(g.decide_shell(&fp2), ProbeShellDecision::Cap);
        assert_eq!(g.shell_rounds(), 4);
        assert_eq!(g.hop_close(), HopClose::Cap);
    }

    #[test]
    fn four_executed_rounds_close_hop_without_fifth_shell() {
        let mut g = HopProbeGovernor::new();
        for round in 1..=4 {
            let fp = tool_probe_fingerprint("shell", &json!({"command": format!("echo {round}")}));
            assert_eq!(g.decide_shell(&fp), ProbeShellDecision::Run);
            g.record_executed_round();
        }
        assert_eq!(g.hop_close(), HopClose::Cap);
        assert_eq!(
            crate::agent::hop_stop::after_hop_close(g.hop_close()),
            crate::agent::hop_stop::AfterHopClose::NextRemainingSkipObserve
        );
    }

    #[test]
    fn governor_skips_repeat_fingerprint() {
        let mut g = HopProbeGovernor::new();
        let fp = tool_probe_fingerprint("shell", &json!({"command": "pwd"}));
        assert_eq!(g.decide_shell(&fp), ProbeShellDecision::Run);
        g.record_executed_round();
        assert_eq!(g.decide_shell(&fp), ProbeShellDecision::SkipRepeat);
        assert_eq!(g.shell_rounds(), 1);
    }

    #[test]
    fn policy_deny_does_not_consume_shell_round() {
        assert!(!shell_output_counts_as_round(
            "[policy_deny] command not in allowlist"
        ));
        assert!(!shell_output_counts_as_round(REPEAT_PROBE_NOTICE));
        assert!(!shell_output_counts_as_round(SHELL_ROUND_CAP_NOTICE));
        assert!(shell_output_counts_as_round("xray.service active"));
        let mut g = HopProbeGovernor::new();
        for i in 0..4 {
            let fp = tool_probe_fingerprint("shell", &json!({"command": format!("denied {i}")}));
            assert_eq!(g.decide_shell(&fp), ProbeShellDecision::Run);
            // deny / skip: do not record
        }
        let fp = tool_probe_fingerprint("shell", &json!({"command": "ssh host.example echo ok"}));
        assert_eq!(g.decide_shell(&fp), ProbeShellDecision::Run);
        g.record_executed_round();
        assert_eq!(g.shell_rounds(), 1);
    }

    #[test]
    fn retract_unexecuted_allows_retry_after_deny() {
        let mut g = HopProbeGovernor::new();
        let fp = tool_probe_fingerprint("shell", &json!({"command": "ssh host.example echo ok"}));
        assert_eq!(g.decide_shell(&fp), ProbeShellDecision::Run);
        g.retract_unexecuted(&fp);
        assert_eq!(g.decide_shell(&fp), ProbeShellDecision::Run);
        g.record_executed_round();
        assert_eq!(g.shell_rounds(), 1);
        assert_eq!(g.decide_shell(&fp), ProbeShellDecision::SkipRepeat);
    }

    #[test]
    fn cap_notice_does_not_teach_handoff() {
        assert!(!SHELL_ROUND_CAP_NOTICE.contains("HANDOFF"));
    }

    #[test]
    fn two_wait_denies_do_not_fail_the_dag() {
        let mut g = HopProbeGovernor::new();
        g.note_shell_output(
            "Command blocked: wait-only executables (sleep/usleep) are not allowed.",
        );
        g.note_shell_output(
            "[policy_deny] Command blocked: wait-only executables (sleep/usleep) are not allowed.",
        );
        assert_eq!(g.hop_close(), HopClose::None);
    }

    #[test]
    fn two_allowlist_denies_do_not_fail_the_dag() {
        let mut g = HopProbeGovernor::new();
        g.note_shell_output("Command not allowed by security policy (not in allowed_commands).");
        assert_eq!(g.hop_close(), HopClose::None);
        g.note_shell_output(
            "[policy_deny] Command not allowed by security policy (not in allowed_commands).",
        );
        assert_eq!(g.hop_close(), HopClose::None);
        assert_eq!(
            crate::agent::hop_stop::after_hop_close(g.hop_close()),
            crate::agent::hop_stop::AfterHopClose::ObserveThenContinue
        );
    }

    #[test]
    fn four_allowlist_denies_cap_hop_and_keep_remaining() {
        let mut g = HopProbeGovernor::new();
        let msg = "[policy_deny] Command not allowed by security policy (not in allowed_commands).";
        for _ in 0..crate::agent::hop_stop::MAX_RECOVERABLE_POLICY_DENIES_BEFORE_CAP {
            g.note_shell_output(msg);
        }
        assert_eq!(g.hop_close(), HopClose::Cap);
        assert_eq!(
            crate::agent::hop_stop::after_hop_close(g.hop_close()),
            crate::agent::hop_stop::AfterHopClose::NextRemainingSkipObserve
        );
    }

    #[test]
    fn two_same_unsafe_denies_close_hop() {
        let mut g = HopProbeGovernor::new();
        g.note_shell_output("unsafe shell construct (injection, redirect, or dangerous args).");
        assert_eq!(g.hop_close(), HopClose::None);
        g.note_shell_output("unsafe shell construct (injection, redirect, or dangerous args).");
        assert_eq!(g.hop_close(), HopClose::PolicyDeny);
    }

    #[test]
    fn unlike_policy_denies_do_not_close() {
        let mut g = HopProbeGovernor::new();
        g.note_shell_output("unsafe shell construct (injection, redirect, or dangerous args).");
        g.note_shell_output("Command not allowed by security policy (not in allowed_commands).");
        assert_eq!(g.hop_close(), HopClose::None);
    }

    #[test]
    fn malformed_or_once_denied_closes_on_first() {
        let mut g = HopProbeGovernor::new();
        g.note_shell_output("[policy_deny] malformed invocation: tool-call carrier in command.");
        assert_eq!(g.hop_close(), HopClose::PolicyDeny);
        let mut g2 = HopProbeGovernor::new();
        g2.note_shell_output("[once_denied] Denied by user after shell-policy approval.");
        assert_eq!(g2.hop_close(), HopClose::PolicyDeny);
        let mut g3 = HopProbeGovernor::new();
        g3.note_shell_output("Denied by user.");
        assert_eq!(g3.hop_close(), HopClose::None);
    }

    #[test]
    fn operator_visible_notices_drop_cap_and_skip_chrome() {
        let kept = operator_visible_notices(vec![
            SHELL_ROUND_CAP_NOTICE.to_string(),
            REPEAT_PROBE_NOTICE.to_string(),
            "### repo-origin\nFour shells ran.".to_string(),
        ]);
        assert!(kept.iter().all(|n| !is_governor_chrome(n)), "{kept:?}");
        assert_eq!(kept.len(), 1);
        assert!(kept[0].contains("Four shells ran."));
    }

    #[test]
    fn hop_close_visible_body_does_not_fill_empty_with_cap_notice() {
        assert!(hop_close_visible_body("", HopClose::Cap).is_empty());
        assert_eq!(
            hop_close_visible_body("envelope gist", HopClose::Cap),
            "envelope gist"
        );
        assert!(hop_close_visible_body("  ", HopClose::PolicyDeny).is_empty());
    }
}
