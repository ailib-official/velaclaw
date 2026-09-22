//! Route honesty helpers (VL-APE-010 / I9–I10).
//! 路由诚实：已执行模型进 Notice；类型化死亡 tombstone；工作跳不被 cost 抢。

use crate::config::ModelRouteConfig;
use crate::providers::hint_peer::HopFailClass;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Mutex;

/// Which Route lane this resolve is for (MS-APE-R2 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TurnLane {
    /// Work LLM / chat_only / parlor. Picker or strongest reachable default.
    #[default]
    WorkCognition,
    /// Judge / planner. `fast` route or session default; cost Decide allowed.
    PlannerJudge,
}

/// User-visible failure that names the executed Route, not picker chrome alone.
#[must_use]
pub fn executed_route_notice(
    kind: &str,
    executed_model: &str,
    route_reason: &str,
    picker: Option<&str>,
) -> String {
    let executed = executed_model.trim();
    let reason = route_reason.trim();
    let picker = picker.map(str::trim).filter(|s| !s.is_empty());
    let mut msg =
        format!("VelaClaw notice: {kind} for executed model `{executed}` (route={reason}).");
    if let Some(p) = picker {
        if p != executed {
            let _ = write!(msg, " Picker was `{p}`.");
        }
    }
    msg
}

/// Drop ids that are session-tombstoned (I10).
#[must_use]
pub fn exclude_tombstoned(session_key: &str, ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| !is_tombstoned(session_key, id))
        .cloned()
        .collect()
}

/// Record a typed provider death for this session (410/404/402).
pub fn tombstone_executed(session_key: &str, model: &str, class: HopFailClass) {
    if !matches!(class, HopFailClass::Unavailable | HopFailClass::Quota) {
        return;
    }
    let m = model.trim();
    let s = session_key.trim();
    if m.is_empty() || s.is_empty() {
        return;
    }
    let mut guard = tombstones().lock().unwrap_or_else(|e| e.into_inner());
    guard
        .entry(s.to_string())
        .or_default()
        .insert(m.to_string());
}

#[must_use]
pub fn is_tombstoned(session_key: &str, model: &str) -> bool {
    let m = model.trim();
    let s = session_key.trim();
    if m.is_empty() || s.is_empty() {
        return false;
    }
    let guard = tombstones().lock().unwrap_or_else(|e| e.into_inner());
    guard.get(s).is_some_and(|set| set.contains(m))
}

/// `[[model_routes]]` hint=fast logical id, if configured.
#[must_use]
pub fn fast_route_logical_id(routes: &[ModelRouteConfig]) -> Option<String> {
    routes.iter().find_map(|r| {
        if !r.hint.trim().eq_ignore_ascii_case("fast") {
            return None;
        }
        let p = r.provider.trim();
        let m = r.model.trim();
        if p.is_empty() || m.is_empty() {
            None
        } else {
            let family = crate::providers::hint_peer::provider_family(p);
            Some(crate::protocol_registry::compose_logical_model_id(
                family, m,
            ))
        }
    })
}

/// Work hop: reachable picker wins; else session default if not tombstoned.
#[must_use]
pub fn work_cognition_model<'a>(
    session_key: &str,
    picker: Option<&'a str>,
    session_default: &'a str,
) -> &'a str {
    if let Some(p) = picker.map(str::trim).filter(|s| !s.is_empty()) {
        if !is_tombstoned(session_key, p) {
            return p;
        }
    }
    session_default
}

/// Host `optimize=cost` must not run on work cognition.
#[must_use]
pub fn cost_optimize_applies_to_lane(lane: TurnLane, optimize: &str) -> bool {
    lane == TurnLane::PlannerJudge && optimize.trim().eq_ignore_ascii_case("cost")
}

/// Prefer host_decide only on the planner lane (cost or otherwise).
#[must_use]
pub fn host_decide_allowed_for_lane(lane: TurnLane) -> bool {
    lane == TurnLane::PlannerJudge
}

fn tombstones() -> &'static Mutex<HashMap<String, HashSet<String>>> {
    static INNER: std::sync::OnceLock<Mutex<HashMap<String, HashSet<String>>>> =
        std::sync::OnceLock::new();
    INNER.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_cites_executed_route_not_picker_only() {
        let msg = executed_route_notice(
            "quota",
            "deepseek/deepseek-v4-flash",
            "host_decide:lowest_cost",
            Some("nvidia/z-ai/glm-5.2"),
        );
        assert!(msg.contains("deepseek/deepseek-v4-flash"), "{msg}");
        assert!(msg.contains("route=host_decide:lowest_cost"), "{msg}");
        assert!(msg.contains("Picker was `nvidia/z-ai/glm-5.2`"), "{msg}");
        assert!(
            msg.contains("executed model `deepseek/deepseek-v4-flash`"),
            "{msg}"
        );
    }

    #[test]
    fn notice_omits_picker_when_same() {
        let msg = executed_route_notice(
            "eol",
            "nvidia/z-ai/glm-5.2",
            "explicit_user_pick",
            Some("nvidia/z-ai/glm-5.2"),
        );
        assert!(!msg.contains("Picker was"), "{msg}");
    }

    #[test]
    fn tombstone_excludes_eol_from_reachable() {
        let sess = "tombstone-unit-sess";
        tombstone_executed(sess, "nvidia/z-ai/glm-5.2", HopFailClass::Unavailable);
        tombstone_executed(sess, "deepseek/deepseek-v4-flash", HopFailClass::Quota);
        let ids = vec![
            "nvidia/z-ai/glm-5.2".into(),
            "nvidia/nemotron-3-super-120b-a12b".into(),
            "deepseek/deepseek-v4-flash".into(),
        ];
        let live = exclude_tombstoned(sess, &ids);
        assert_eq!(live, vec!["nvidia/nemotron-3-super-120b-a12b".to_string()]);
        assert!(is_tombstoned(sess, "nvidia/z-ai/glm-5.2"));
        assert!(!is_tombstoned(sess, "nvidia/nemotron-3-super-120b-a12b"));
    }

    #[test]
    fn cost_optimize_does_not_steal_work_hop() {
        assert!(!cost_optimize_applies_to_lane(
            TurnLane::WorkCognition,
            "cost"
        ));
        assert!(!host_decide_allowed_for_lane(TurnLane::WorkCognition));
        assert!(host_decide_allowed_for_lane(TurnLane::PlannerJudge));
        assert!(cost_optimize_applies_to_lane(
            TurnLane::PlannerJudge,
            "cost"
        ));
        let sess = "work-hop-cost-sess";
        tombstone_executed(sess, "nvidia/z-ai/glm-5.2", HopFailClass::Unavailable);
        assert_eq!(
            work_cognition_model(sess, Some("nvidia/z-ai/glm-5.2"), "nvidia/super"),
            "nvidia/super"
        );
        assert_eq!(
            work_cognition_model(sess, Some("nvidia/super"), "cheap/flash"),
            "nvidia/super"
        );
    }

    #[test]
    fn live_planner_uses_cognition_not_fast() {
        let routes = vec![
            ModelRouteConfig {
                hint: "code".into(),
                provider: "nvidia".into(),
                model: "nemotron-3-super-120b-a12b".into(),
                ..ModelRouteConfig::default()
            },
            ModelRouteConfig {
                hint: "fast".into(),
                provider: "groq".into(),
                model: "openai/gpt-oss-20b".into(),
                ..ModelRouteConfig::default()
            },
        ];
        assert_eq!(
            fast_route_logical_id(&routes).as_deref(),
            Some("groq/openai/gpt-oss-20b")
        );
        let doubled = vec![ModelRouteConfig {
            hint: "fast".into(),
            provider: "groq/openai/gpt-oss-20b".into(),
            model: "openai/gpt-oss-20b".into(),
            ..ModelRouteConfig::default()
        }];
        assert_eq!(
            fast_route_logical_id(&doubled).as_deref(),
            Some("groq/openai/gpt-oss-20b")
        );
        assert_eq!(
            crate::agent::capability_route::strong_planner_model(
                "nvidia/super",
                fast_route_logical_id(&routes).as_deref(),
            ),
            Some("nvidia/super")
        );
        assert_eq!(
            crate::agent::capability_route::strong_planner_model(
                "groq/openai/gpt-oss-20b",
                fast_route_logical_id(&routes).as_deref(),
            ),
            None
        );
    }
}
