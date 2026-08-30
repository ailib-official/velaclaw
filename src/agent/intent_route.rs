//! CR-CAP-003/005: opt-in capability-index route (Hint/Tag → reachable ∩ constraints).
//!
//! Product narrative (CR-CAP-005 / MS-HOST-CAP-R1): **capability-index routing**, not
//! NL "intent routing" as the main story. Tag may come from explicit Tag / Hint;
//! `query_classification` is optional only.
//!
//! Default-off. Uses CR-CAP-004 query-time reachable filter (local keys / keyless).
//! Empty reachable sets after constraints fail closed (no silent default_model).
//! UnrelatedWire Tags (e.g. `speed`) may use an explicit `[[model_routes]]`
//! entry when the index is empty by design — still not an arbitrary fallback.

use crate::agent::classifier;
use crate::capability_index::{
    filter_reachable, lookup_tag, CapabilityCandidate, CapabilityIndex, TagWireRelation,
    CAPABILITY_TAGS, TAG_MAPPING_TABLE,
};
use crate::config::{ModelRouteConfig, QueryClassificationConfig};
use crate::execution::provider_has_usable_key;
use anyhow::{bail, Result};
use serde::Serialize;

/// L0 Hint → Capability Tag (plans `capability-mapping.md` §2 + direct Tags).
const HINT_TO_TAG: &[(&str, &str)] = &[
    ("reasoning", "high-reasoning"),
    ("high-reasoning", "high-reasoning"),
    ("fast", "speed"),
    ("speed", "speed"),
    ("code", "coding"),
    ("coding", "coding"),
    ("document", "document_understanding"),
    ("document_understanding", "document_understanding"),
    ("tools", "tool_calling"),
    ("tool_calling", "tool_calling"),
    ("long_context", "long_context"),
    ("long-context", "long_context"),
];

/// Host knobs for the opt-in capability-index route (default-off).
#[derive(Debug, Clone)]
pub struct IntentRouteHost {
    pub enabled: bool,
    pub config_dir: std::path::PathBuf,
    pub model_routes: Vec<ModelRouteConfig>,
}

impl IntentRouteHost {
    pub fn from_config(config: &crate::config::Config) -> Self {
        let config_dir = config
            .config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        Self {
            enabled: config.agent.intent_capability_route,
            config_dir,
            model_routes: config.model_routes.clone(),
        }
    }
}

/// Explainable decision record (facts + reachable + policy reasons).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IntentRouteDecision {
    pub enabled: bool,
    pub hint: Option<String>,
    pub tags: Vec<String>,
    /// Declared CAP-002 candidates before reachable/constraint filters.
    pub candidates_before: usize,
    /// CAP-004 reachable count (keys ∩ declared) before `[[model_routes]]` constraints.
    pub reachable_before: usize,
    pub candidates_after: usize,
    pub truncated: Vec<String>,
    pub selected_model: Option<String>,
    pub reason: String,
    pub fail_closed: bool,
}

impl IntentRouteDecision {
    #[must_use]
    pub fn skipped_prior_path(model: &str, reason: &str) -> Self {
        Self {
            enabled: false,
            hint: None,
            tags: Vec::new(),
            candidates_before: 0,
            reachable_before: 0,
            candidates_after: 0,
            truncated: Vec::new(),
            selected_model: Some(model.to_string()),
            reason: reason.to_string(),
            fail_closed: false,
        }
    }
}

/// Map a VC hint (or Tag name) to a Capability Tag.
#[must_use]
pub fn hint_to_tag(hint: &str) -> Option<&'static str> {
    let h = hint.trim();
    if h.is_empty() {
        return None;
    }
    if let Some(tag) = CAPABILITY_TAGS.iter().copied().find(|t| *t == h) {
        return Some(tag);
    }
    HINT_TO_TAG
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(h))
        .map(|(_, tag)| *tag)
}

/// Hints in `HINT_TO_TAG` that map to this Capability Tag (for node Contact).
#[must_use]
pub fn hints_for_tag(tag: &str) -> Vec<&'static str> {
    HINT_TO_TAG
        .iter()
        .filter(|(_, t)| t.eq_ignore_ascii_case(tag))
        .map(|(h, _)| *h)
        .collect()
}

fn tag_relation(tag: &str) -> Option<TagWireRelation> {
    TAG_MAPPING_TABLE
        .iter()
        .find(|e| e.tag == tag)
        .map(|e| e.relation)
}

fn route_for_hint<'a>(routes: &'a [ModelRouteConfig], hint: &str) -> Option<&'a ModelRouteConfig> {
    routes.iter().find(|r| r.hint == hint)
}

fn candidate_label(c: &CapabilityCandidate) -> String {
    c.logical_model_id
        .clone()
        .unwrap_or_else(|| format!("{}/*", c.provider_id))
}

fn logical_model_for_candidate(
    c: &CapabilityCandidate,
    route: Option<&ModelRouteConfig>,
) -> String {
    if let Some(id) = &c.logical_model_id {
        return id.clone();
    }
    if let Some(route) = route {
        if route.provider == c.provider_id {
            if route.model.contains('/') {
                return route.model.clone();
            }
            return format!("{}/{}", route.provider, route.model);
        }
    }
    if let Some(route) = route {
        if route.model.contains('/') {
            return route.model.clone();
        }
        return format!("{}/{}", route.provider, route.model);
    }
    format!("{}/default", c.provider_id)
}

fn apply_constraints<'a>(
    candidates: &'a [CapabilityCandidate],
    route: Option<&ModelRouteConfig>,
) -> Vec<&'a CapabilityCandidate> {
    let Some(route) = route else {
        return candidates.iter().collect();
    };
    let matched: Vec<_> = candidates
        .iter()
        .filter(|c| {
            if c.provider_id != route.provider {
                return false;
            }
            match &c.logical_model_id {
                None => true,
                Some(id) => {
                    id == &route.model
                        || id.ends_with(&format!("/{}", route.model))
                        || route.model.ends_with(id.as_str())
                        || id == &format!("{}/{}", route.provider, route.model)
                }
            }
        })
        .collect();
    if matched.is_empty() {
        candidates
            .iter()
            .filter(|c| c.provider_id == route.provider)
            .collect()
    } else {
        matched
    }
}

/// Core resolve against a preloaded index (unit-test friendly).
///
/// `is_reachable`: CR-CAP-004 predicate (typically `provider_has_usable_key`).
/// `explicit_tag`: when set, skip NL classification / hint mapping and use this Tag.
/// `explicit_hint`: Hint or Tag name; used when `explicit_tag` is unset.
pub fn resolve_with_index(
    index: &CapabilityIndex,
    classification: &QueryClassificationConfig,
    available_hints: &[String],
    model_routes: &[ModelRouteConfig],
    default_model: &str,
    user_message: &str,
    explicit_hint: Option<&str>,
    explicit_tag: Option<&str>,
    is_reachable: impl Fn(&str) -> bool,
) -> Result<IntentRouteDecision> {
    let _ = available_hints;

    let (hint_label, tag) = if let Some(raw_tag) = explicit_tag {
        let Some(tag) = hint_to_tag(raw_tag) else {
            bail!("capability route fail-closed: unknown Tag '{raw_tag}' (capability-mapping.md)");
        };
        (Some(tag.to_string()), tag)
    } else {
        let hint = explicit_hint
            .map(str::to_string)
            .or_else(|| classifier::classify(classification, user_message));

        let Some(hint) = hint else {
            return Ok(IntentRouteDecision {
                enabled: true,
                hint: None,
                tags: Vec::new(),
                candidates_before: 0,
                reachable_before: 0,
                candidates_after: 0,
                truncated: Vec::new(),
                selected_model: Some(default_model.to_string()),
                reason: "no Tag/hint matched; using default model (not a Tag empty-set)".into(),
                fail_closed: false,
            });
        };

        let Some(tag) = hint_to_tag(&hint) else {
            bail!(
                "capability route fail-closed: hint '{hint}' has no Capability Tag mapping (capability-mapping.md)"
            );
        };
        (Some(hint), tag)
    };

    let route = hint_label
        .as_deref()
        .and_then(|h| route_for_hint(model_routes, h))
        .or_else(|| route_for_hint(model_routes, tag));

    let declared = lookup_tag(index, tag)?;
    let declared_before = declared.len();
    let reachable_refs = filter_reachable(declared, &is_reachable);
    let reachable_before = reachable_refs.len();
    let reachable: Vec<CapabilityCandidate> = reachable_refs.into_iter().cloned().collect();
    let filtered = apply_constraints(&reachable, route);
    let after = filtered.len();
    let truncated: Vec<String> = filtered
        .iter()
        .take(8)
        .map(|c| candidate_label(c))
        .collect();

    if after == 0 {
        if tag_relation(tag) == Some(TagWireRelation::UnrelatedWire) {
            if let Some(route) = route {
                let selected = if route.model.contains('/') {
                    route.model.clone()
                } else {
                    format!("{}/{}", route.provider, route.model)
                };
                return Ok(IntentRouteDecision {
                    enabled: true,
                    hint: hint_label.clone(),
                    tags: vec![tag.to_string()],
                    candidates_before: declared_before,
                    reachable_before,
                    candidates_after: 0,
                    truncated: vec![selected.clone()],
                    selected_model: Some(selected),
                    reason: format!(
                        "Tag '{tag}' is unrelated_wire (index empty by design); using [[model_routes]] for '{}'",
                        hint_label.as_deref().unwrap_or(tag)
                    ),
                    fail_closed: false,
                });
            }
        }
        return Ok(IntentRouteDecision {
            enabled: true,
            hint: hint_label.clone(),
            tags: vec![tag.to_string()],
            candidates_before: declared_before,
            reachable_before,
            candidates_after: 0,
            truncated: Vec::new(),
            selected_model: None,
            reason: format!(
                "fail-closed: Tag '{tag}' yielded empty reachable∩constraints \
                 (declared={declared_before}, reachable={reachable_before}; hint {:?})",
                hint_label
            ),
            fail_closed: true,
        });
    }

    let chosen = filtered[0];
    let selected = logical_model_for_candidate(chosen, route);
    Ok(IntentRouteDecision {
        enabled: true,
        hint: hint_label,
        tags: vec![tag.to_string()],
        candidates_before: declared_before,
        reachable_before,
        candidates_after: after,
        truncated,
        selected_model: Some(selected),
        reason: format!(
            "reachable∩constraints: Tag '{tag}' → {} (of {after} after filter; \
             declared={declared_before}, reachable={reachable_before})",
            candidate_label(chosen)
        ),
        fail_closed: false,
    })
}

/// Resolve when host flag is on; load/rebuild CAP-002 index as needed.
pub fn resolve_for_host(
    host: &IntentRouteHost,
    classification: &QueryClassificationConfig,
    available_hints: &[String],
    default_model: &str,
    user_message: &str,
    explicit_hint: Option<&str>,
    explicit_tag: Option<&str>,
    rebuild_index: bool,
) -> Result<IntentRouteDecision> {
    if !host.enabled {
        let model = classifier::resolve_model_for_message(
            classification,
            available_hints,
            default_model,
            user_message,
        );
        return Ok(IntentRouteDecision::skipped_prior_path(
            &model,
            "capability_index_route / intent_capability_route disabled; prior classification/default path",
        ));
    }

    let (index, _) =
        crate::capability_index::load_or_rebuild_for_config(&host.config_dir, rebuild_index)?;
    let decision = resolve_with_index(
        &index,
        classification,
        available_hints,
        &host.model_routes,
        default_model,
        user_message,
        explicit_hint,
        explicit_tag,
        provider_has_usable_key,
    )?;
    if decision.fail_closed {
        bail!("{}", decision.reason);
    }
    tracing::info!(
        hint = ?decision.hint,
        tags = ?decision.tags,
        selected = ?decision.selected_model,
        declared = decision.candidates_before,
        reachable = decision.reachable_before,
        reason = %decision.reason,
        "capability_index_route"
    );
    Ok(decision)
}

/// Library helper: `Ok(None)` when disabled; otherwise decision (errors on fail-closed).
pub fn maybe_resolve_intent_route(
    host: &IntentRouteHost,
    classification: &QueryClassificationConfig,
    available_hints: &[String],
    default_model: &str,
    user_message: &str,
) -> Result<Option<IntentRouteDecision>> {
    if !host.enabled {
        return Ok(None);
    }
    Ok(Some(resolve_for_host(
        host,
        classification,
        available_hints,
        default_model,
        user_message,
        None,
        None,
        false,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_index::build_index;
    use crate::config::schema::{ClassificationRule, QueryClassificationConfig};
    use std::fs;

    fn write_provider(dir: &std::path::Path, name: &str, body: &str) {
        let providers = dir.join("v2").join("providers");
        fs::create_dir_all(&providers).unwrap();
        fs::write(providers.join(name), body).unwrap();
    }

    fn class_cfg(hint: &str, keyword: &str) -> QueryClassificationConfig {
        QueryClassificationConfig {
            enabled: true,
            rules: vec![ClassificationRule {
                hint: hint.into(),
                keywords: vec![keyword.into()],
                patterns: vec![],
                min_length: None,
                max_length: None,
                priority: 0,
            }],
        }
    }

    fn all_reachable(_: &str) -> bool {
        true
    }

    fn resolve(
        index: &CapabilityIndex,
        class: &QueryClassificationConfig,
        hints: &[String],
        routes: &[ModelRouteConfig],
        default: &str,
        msg: &str,
        hint: Option<&str>,
        tag: Option<&str>,
    ) -> IntentRouteDecision {
        resolve_with_index(
            index,
            class,
            hints,
            routes,
            default,
            msg,
            hint,
            tag,
            all_reachable,
        )
        .unwrap()
    }

    #[test]
    fn flag_off_style_skipped_decision() {
        let d = IntentRouteDecision::skipped_prior_path("openai/gpt-5.2", "disabled");
        assert!(!d.enabled);
        assert_eq!(d.selected_model.as_deref(), Some("openai/gpt-5.2"));
        assert!(!d.fail_closed);
    }

    #[test]
    fn coding_selects_from_index() {
        let dir = tempfile::tempdir().unwrap();
        write_provider(
            dir.path(),
            "alpha.json",
            r#"{"id":"alpha","capabilities":{"required":["tools"],"optional":["reasoning"]},"metadata":{"models":{"big":{"context_window":200000}}}}"#,
        );
        let index = build_index(dir.path()).unwrap();
        let decision = resolve(
            &index,
            &class_cfg("coding", "refactor"),
            &["coding".into()],
            &[],
            "openai/gpt-5.2",
            "please refactor this module",
            None,
            None,
        );
        assert!(!decision.fail_closed);
        assert_eq!(decision.tags, vec!["coding"]);
        assert!(decision
            .selected_model
            .as_ref()
            .unwrap()
            .starts_with("alpha/"));
        assert!(decision.candidates_after >= 1);
        assert_eq!(decision.reachable_before, decision.candidates_before);
    }

    #[test]
    fn explicit_tag_skips_classification() {
        let dir = tempfile::tempdir().unwrap();
        write_provider(
            dir.path(),
            "alpha.json",
            r#"{"id":"alpha","capabilities":{"required":["tools"],"optional":[]}}"#,
        );
        let index = build_index(dir.path()).unwrap();
        let decision = resolve(
            &index,
            &QueryClassificationConfig {
                enabled: false,
                rules: vec![],
            },
            &[],
            &[],
            "openai/gpt-5.2",
            "unrelated chatter without keywords",
            None,
            Some("coding"),
        );
        assert!(!decision.fail_closed);
        assert_eq!(decision.tags, vec!["coding"]);
        assert!(decision
            .selected_model
            .as_ref()
            .unwrap()
            .starts_with("alpha/"));
    }

    #[test]
    fn unreachable_providers_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        write_provider(
            dir.path(),
            "alpha.json",
            r#"{"id":"alpha","capabilities":{"required":["tools"],"optional":[]}}"#,
        );
        let index = build_index(dir.path()).unwrap();
        let decision = resolve_with_index(
            &index,
            &class_cfg("coding", "refactor"),
            &["coding".into()],
            &[],
            "openai/gpt-5.2",
            "please refactor",
            None,
            Some("coding"),
            |_| false,
        )
        .unwrap();
        assert!(decision.fail_closed);
        assert_eq!(decision.candidates_before, 1);
        assert_eq!(decision.reachable_before, 0);
        assert!(decision.selected_model.is_none());
    }

    #[test]
    fn empty_after_constraints_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        write_provider(
            dir.path(),
            "alpha.json",
            r#"{"id":"alpha","capabilities":{"required":["tools"],"optional":[]}}"#,
        );
        let index = build_index(dir.path()).unwrap();
        let routes = vec![ModelRouteConfig {
            hint: "coding".into(),
            provider: "beta".into(),
            model: "beta/x".into(),
            api_key: None,
            fallbacks: Vec::new(),
        }];
        let decision = resolve(
            &index,
            &class_cfg("coding", "refactor"),
            &["coding".into()],
            &routes,
            "openai/gpt-5.2",
            "please refactor",
            None,
            None,
        );
        assert!(decision.fail_closed);
        assert!(decision.selected_model.is_none());
    }

    #[test]
    fn speed_unrelated_wire_uses_model_routes() {
        let dir = tempfile::tempdir().unwrap();
        write_provider(
            dir.path(),
            "alpha.json",
            r#"{"id":"alpha","capabilities":{"required":["text"],"optional":[]}}"#,
        );
        let index = build_index(dir.path()).unwrap();
        let routes = vec![ModelRouteConfig {
            hint: "fast".into(),
            provider: "groq".into(),
            model: "llama-fast".into(),
            api_key: None,
            fallbacks: Vec::new(),
        }];
        let decision = resolve(
            &index,
            &class_cfg("fast", "quick"),
            &["fast".into()],
            &routes,
            "openai/gpt-5.2",
            "quick ping",
            None,
            None,
        );
        assert!(!decision.fail_closed);
        assert_eq!(decision.selected_model.as_deref(), Some("groq/llama-fast"));
        assert!(decision.reason.contains("unrelated_wire"));
    }

    #[test]
    fn speed_without_routes_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        write_provider(
            dir.path(),
            "alpha.json",
            r#"{"id":"alpha","capabilities":{"required":["text"],"optional":[]}}"#,
        );
        let index = build_index(dir.path()).unwrap();
        let decision = resolve(
            &index,
            &class_cfg("fast", "quick"),
            &["fast".into()],
            &[],
            "openai/gpt-5.2",
            "quick ping",
            None,
            None,
        );
        assert!(decision.fail_closed);
    }

    #[test]
    fn unknown_hint_errors() {
        let dir = tempfile::tempdir().unwrap();
        let index = build_index(dir.path()).unwrap();
        let err = resolve_with_index(
            &index,
            &class_cfg("summarize", "tl;dr"),
            &["summarize".into()],
            &[],
            "openai/gpt-5.2",
            "tl;dr please",
            None,
            None,
            all_reachable,
        )
        .unwrap_err();
        assert!(err.to_string().contains("fail-closed"));
    }
}
