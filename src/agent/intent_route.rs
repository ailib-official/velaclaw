//! CR-CAP-003: opt-in intent → Tag → capability-index → constraints route.
//!
//! Default-off. Empty candidate sets fail closed (no silent default_model).
//! UnrelatedWire Tags (e.g. `speed`) may use an explicit `[[model_routes]]`
//! entry when the index is empty by design — still not an arbitrary fallback.

use crate::agent::classifier;
use crate::capability_index::{
    lookup_tag, CapabilityCandidate, CapabilityIndex, TagWireRelation, CAPABILITY_TAGS,
    TAG_MAPPING_TABLE,
};
use crate::config::{ModelRouteConfig, QueryClassificationConfig};
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

/// Host knobs for the opt-in intent route (default-off).
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

/// Explainable decision record (facts + policy reasons).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IntentRouteDecision {
    pub enabled: bool,
    pub hint: Option<String>,
    pub tags: Vec<String>,
    pub candidates_before: usize,
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
            // Prefer configured model when provider matches a provider-level Tag hit.
            if route.model.contains('/') {
                return route.model.clone();
            }
            return format!("{}/{}", route.provider, route.model);
        }
    }
    // Provider-level hit without a concrete model — keep hint form for RouterProvider
    // only when a route exists; otherwise provider id alone is not routable.
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
        // Soften: provider match only when model-level intersect is empty.
        candidates
            .iter()
            .filter(|c| c.provider_id == route.provider)
            .collect()
    } else {
        matched
    }
}

/// Core resolve against a preloaded index (unit-test friendly).
pub fn resolve_with_index(
    index: &CapabilityIndex,
    classification: &QueryClassificationConfig,
    available_hints: &[String],
    model_routes: &[ModelRouteConfig],
    default_model: &str,
    user_message: &str,
    explicit_hint: Option<&str>,
) -> Result<IntentRouteDecision> {
    let hint = explicit_hint
        .map(str::to_string)
        .or_else(|| classifier::classify(classification, user_message));

    let Some(hint) = hint else {
        return Ok(IntentRouteDecision {
            enabled: true,
            hint: None,
            tags: Vec::new(),
            candidates_before: 0,
            candidates_after: 0,
            truncated: Vec::new(),
            selected_model: Some(default_model.to_string()),
            reason: "no intent/hint matched; using default model (not a Tag empty-set)".into(),
            fail_closed: false,
        });
    };

    let Some(tag) = hint_to_tag(&hint) else {
        bail!(
            "intent route fail-closed: hint '{hint}' has no Capability Tag mapping (capability-mapping.md)"
        );
    };

    let route = route_for_hint(model_routes, &hint).or_else(|| route_for_hint(model_routes, tag));

    // Classification without a model_routes entry is allowed when the index has
    // candidates; available_hints only gates the legacy hint: path.
    let _ = available_hints;

    let candidates = lookup_tag(index, tag)?;
    let before = candidates.len();
    let filtered = apply_constraints(candidates, route);
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
                    hint: Some(hint.clone()),
                    tags: vec![tag.to_string()],
                    candidates_before: before,
                    candidates_after: 0,
                    truncated: vec![selected.clone()],
                    selected_model: Some(selected),
                    reason: format!(
                        "Tag '{tag}' is unrelated_wire (index empty by design); using [[model_routes]] for hint '{hint}'"
                    ),
                    fail_closed: false,
                });
            }
        }
        return Ok(IntentRouteDecision {
            enabled: true,
            hint: Some(hint.clone()),
            tags: vec![tag.to_string()],
            candidates_before: before,
            candidates_after: 0,
            truncated: Vec::new(),
            selected_model: None,
            reason: format!(
                "fail-closed: Tag '{tag}' yielded empty candidates after constraints (hint '{hint}')"
            ),
            fail_closed: true,
        });
    }

    let chosen = filtered[0];
    let selected = logical_model_for_candidate(chosen, route);
    Ok(IntentRouteDecision {
        enabled: true,
        hint: Some(hint),
        tags: vec![tag.to_string()],
        candidates_before: before,
        candidates_after: after,
        truncated,
        selected_model: Some(selected),
        reason: format!(
            "index∩constraints: Tag '{tag}' → {} (of {after} after filter; {before} before)",
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
            "intent_capability_route disabled; prior classification/default path",
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
    )?;
    if decision.fail_closed {
        bail!("{}", decision.reason);
    }
    tracing::info!(
        hint = ?decision.hint,
        tags = ?decision.tags,
        selected = ?decision.selected_model,
        reason = %decision.reason,
        "intent_capability_route"
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
                ..Default::default()
            }],
        }
    }

    #[test]
    fn hint_mapping_covers_l0_examples() {
        assert_eq!(hint_to_tag("reasoning"), Some("high-reasoning"));
        assert_eq!(hint_to_tag("fast"), Some("speed"));
        assert_eq!(hint_to_tag("coding"), Some("coding"));
        assert_eq!(hint_to_tag("nope"), None);
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
        let decision = resolve_with_index(
            &index,
            &class_cfg("coding", "refactor"),
            &["coding".into()],
            &[],
            "openai/gpt-5.2",
            "please refactor this module",
            None,
        )
        .unwrap();
        assert!(!decision.fail_closed);
        assert_eq!(decision.tags, vec!["coding"]);
        assert!(decision
            .selected_model
            .as_ref()
            .unwrap()
            .starts_with("alpha/"));
        assert!(decision.candidates_after >= 1);
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
        }];
        let decision = resolve_with_index(
            &index,
            &class_cfg("coding", "refactor"),
            &["coding".into()],
            &routes,
            "openai/gpt-5.2",
            "please refactor",
            None,
        )
        .unwrap();
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
        }];
        let decision = resolve_with_index(
            &index,
            &class_cfg("fast", "quick"),
            &["fast".into()],
            &routes,
            "openai/gpt-5.2",
            "quick ping",
            None,
        )
        .unwrap();
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
        let decision = resolve_with_index(
            &index,
            &class_cfg("fast", "quick"),
            &["fast".into()],
            &[],
            "openai/gpt-5.2",
            "quick ping",
            None,
        )
        .unwrap();
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
        )
        .unwrap_err();
        assert!(err.to_string().contains("fail-closed"));
    }
}
