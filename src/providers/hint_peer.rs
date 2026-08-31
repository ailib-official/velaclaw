//! Per-hint peer fallback after micro-retry (VL-NA-021).
//! hint 级平级切换：同 hop 微重试耗尽后换列表内下一模型。

use std::collections::{HashMap, HashSet};

/// Total model tries on one hop (including the first).
pub const MAX_PEER_ATTEMPTS: usize = 5;
/// How many times the provider family may change on one hop.
pub const MAX_CROSS_PROVIDER: usize = 3;

/// First path segment of a logical provider id (`nvidia/…` → `nvidia`).
#[must_use]
pub fn provider_family(provider: &str) -> &str {
    provider
        .split('/')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(provider)
}

/// Why this hop failed — used for peer switch and DAG fail strategy (VL-NA-022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HopFailClass {
    /// Vendor model missing / EOL / account has no such function.
    Unavailable,
    /// 429 / 402-style quota.
    Quota,
    /// DNS / connect — switching providers will not help.
    Transport,
    /// Sandbox / path / approval — not a model problem.
    Policy,
    Other,
}

impl HopFailClass {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Quota => "quota",
            Self::Transport => "transport",
            Self::Policy => "policy",
            Self::Other => "other",
        }
    }
}

/// Classify a provider/tool error for hop fallback (not one HTTP code at a time).
#[must_use]
pub fn classify_hop_error(err: &str) -> HopFailClass {
    let lower = err.to_lowercase();
    if looks_like_transport(&lower) {
        return HopFailClass::Transport;
    }
    if looks_like_policy(&lower) {
        return HopFailClass::Policy;
    }
    if looks_like_rate_or_quota(&lower) {
        return HopFailClass::Quota;
    }
    if looks_like_model_retired(&lower) || looks_like_model_unavailable(&lower) {
        return HopFailClass::Unavailable;
    }
    HopFailClass::Other
}

/// True when the hop should try the next hint peer (not DNS/transport).
#[must_use]
pub fn is_peer_switchable(err: &str) -> bool {
    matches!(
        classify_hop_error(err),
        HopFailClass::Unavailable | HopFailClass::Quota
    )
}

/// HTTP 410 / vendor EOL — not billing.
#[must_use]
pub fn looks_like_model_retired(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("end of life")
        || lower.contains("http 410")
        || lower.contains("status\":410")
        || (lower.contains("410") && (lower.contains("gone") || lower.contains("http_error")))
}

fn looks_like_transport(lower: &str) -> bool {
    lower.contains("dns error")
        || lower.contains("failed to lookup address")
        || lower.contains("error trying to connect")
        || lower.contains("network transport")
}

fn looks_like_policy(lower: &str) -> bool {
    lower.contains("[policy_deny]")
        || lower.contains("[sandbox_deny]")
        || lower.contains("[needs_approval]")
        || lower.contains("path not allowed by security policy")
        || lower.contains("not in allowed_commands")
}

/// Account/catalog miss: HTTP 404 Function Not Found, model_not_found.
/// Does **not** match workspace "file not found".
fn looks_like_model_unavailable(lower: &str) -> bool {
    if lower.contains("file not found") || lower.contains("no such file") {
        return false;
    }
    if lower.contains("model_not_found") {
        return true;
    }
    let http_404 = lower.contains("http 404")
        || lower.contains("status\":404")
        || lower.contains("status 404");
    if !http_404 {
        return false;
    }
    lower.contains("not_found")
        || lower.contains("function")
        || lower.contains("does not exist")
        || lower.contains("model")
        || lower.contains("not found")
}

fn looks_like_rate_or_quota(lower: &str) -> bool {
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limited")
        || lower.contains("insufficient quota")
        || lower.contains("insufficient_quota")
        || lower.contains("insufficient balance")
        || lower.contains("out of credits")
}

/// Session pin + blacklist for one RouterProvider (one agent session).
#[derive(Debug, Default)]
pub struct HintPeerSession {
    /// hint → pinned model id after a successful peer hop
    pub pinned: HashMap<String, String>,
    pub blacklisted: HashSet<String>,
}

impl HintPeerSession {
    pub fn blacklist(&mut self, model: &str) {
        let m = model.trim();
        if !m.is_empty() {
            self.blacklisted.insert(m.to_string());
        }
    }

    pub fn pin(&mut self, hint: &str, model: &str) {
        let h = hint.trim();
        let m = model.trim();
        if !h.is_empty() && !m.is_empty() {
            self.pinned.insert(h.to_string(), m.to_string());
        }
    }
}

/// One (provider, model) pair in a hint chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintPeerCandidate {
    pub provider_name: String,
    pub model: String,
}

/// Order candidates for this hop: pinned first (if still listed), else chain minus blacklist.
#[must_use]
pub fn ordered_candidates(
    hint: &str,
    chain: &[HintPeerCandidate],
    session: &HintPeerSession,
) -> Vec<HintPeerCandidate> {
    let hint = hint.trim();
    if let Some(pinned) = session.pinned.get(hint) {
        return chain
            .iter()
            .filter(|c| c.model == *pinned && !session.blacklisted.contains(&c.model))
            .cloned()
            .collect();
    }
    chain
        .iter()
        .filter(|c| !session.blacklisted.contains(&c.model))
        .cloned()
        .collect()
}

/// Apply attempt / cross-provider caps; `continue` skips a candidate that would exceed cross.
#[must_use]
pub fn admit_attempt(
    prev_family: Option<&str>,
    next_family: &str,
    attempts: usize,
    cross_used: usize,
) -> Admit {
    if attempts >= MAX_PEER_ATTEMPTS {
        return Admit::Stop;
    }
    match prev_family {
        Some(prev) if prev != next_family => {
            if cross_used >= MAX_CROSS_PROVIDER {
                Admit::Skip
            } else {
                Admit::Take { cross_delta: 1 }
            }
        }
        _ => Admit::Take { cross_delta: 0 },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    Take { cross_delta: usize },
    Skip,
    Stop,
}

/// User-facing notice when a hop switched models.
#[must_use]
pub fn hint_peer_switch_notice(
    hint: &str,
    from_model: &str,
    to_model: &str,
    reason: &str,
) -> String {
    format!(
        "VelaClaw notice: hint `{hint}` switched from `{from_model}` to `{to_model}` ({reason})."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_is_not_switchable() {
        assert!(!is_peer_switchable(
            "Network transport error: dns error: failed to lookup address"
        ));
        assert!(is_peer_switchable(
            "HTTP 410 (http_error): Gone end of life"
        ));
        assert!(is_peer_switchable("HTTP 429 rate limit"));
        assert!(is_peer_switchable("error: model_not_found"));
        assert!(!is_peer_switchable("tool failed: file not found"));
        assert!(is_peer_switchable(
            "HTTP 404 (not_found): Function xyz does not exist on this NGC account"
        ));
        assert_eq!(
            classify_hop_error("HTTP 404 (not_found): Function missing"),
            HopFailClass::Unavailable
        );
        assert_eq!(
            classify_hop_error("Path not allowed by security policy: /tmp/x"),
            HopFailClass::Policy
        );
        assert_eq!(
            classify_hop_error("Network transport error: dns error"),
            HopFailClass::Transport
        );
    }

    #[test]
    fn pin_skips_primary_after_success() {
        let chain = vec![
            HintPeerCandidate {
                provider_name: "nvidia".into(),
                model: "dead".into(),
            },
            HintPeerCandidate {
                provider_name: "nvidia".into(),
                model: "live".into(),
            },
        ];
        let mut session = HintPeerSession::default();
        session.blacklist("dead");
        session.pin("reasoning", "live");
        let ordered = ordered_candidates("reasoning", &chain, &session);
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].model, "live");
    }

    #[test]
    fn cross_provider_cap_skips() {
        assert_eq!(admit_attempt(Some("nvidia"), "deepseek", 1, 3), Admit::Skip);
        assert_eq!(
            admit_attempt(Some("nvidia"), "nvidia", 1, 3),
            Admit::Take { cross_delta: 0 }
        );
        assert_eq!(admit_attempt(None, "nvidia", 5, 0), Admit::Stop);
    }
}
