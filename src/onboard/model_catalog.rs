//! Manifest-first model catalog for onboard (VL-REVIEW-001).
//!
//! When `AI_PROTOCOL_DIR` / local protocol root is available, model lists are
//! derived from [`crate::protocol_registry`]. Otherwise callers keep using their
//! curated offline fallback tables.

use crate::config::DEFAULT_PROTOCOL_MODEL_ID;
#[cfg(feature = "ai-protocol")]
use crate::protocol_registry::{self, ProtocolRegistrySnapshot};
use std::sync::OnceLock;

#[cfg(feature = "ai-protocol")]
fn cached_registry() -> Option<&'static ProtocolRegistrySnapshot> {
    static REGISTRY: OnceLock<Option<ProtocolRegistrySnapshot>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let root = protocol_registry::resolve_local_protocol_root()?;
            match protocol_registry::scan_protocol_root(&root) {
                Ok(snap) => Some(snap),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        path = %root.display(),
                        "failed to scan ai-protocol root for onboard catalog"
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// Models for a provider from the protocol registry, as `(id, label)` pairs.
///
/// Returns `None` when the ai-protocol feature is off, no protocol root is
/// found, or the provider has no registered models (caller should use curated fallback).
pub fn models_from_manifest(provider: &str) -> Option<Vec<(String, String)>> {
    #[cfg(feature = "ai-protocol")]
    {
        let snap = cached_registry()?;
        let provider_l = provider.to_ascii_lowercase();
        let mut out: Vec<(String, String)> = snap
            .models
            .iter()
            .filter(|m| {
                m.provider.eq_ignore_ascii_case(&provider_l)
                    || m.logical_id
                        .split_once('/')
                        .is_some_and(|(p, _)| p.eq_ignore_ascii_case(&provider_l))
            })
            .map(|m| {
                let label = format!("{} (from ai-protocol)", m.logical_id);
                (m.logical_id.clone(), label)
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.dedup_by(|a, b| a.0 == b.0);
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
    #[cfg(not(feature = "ai-protocol"))]
    {
        let _ = provider;
        None
    }
}

/// Default model id for a provider from the protocol registry.
///
/// Prefers [`DEFAULT_PROTOCOL_MODEL_ID`] when it belongs to the provider;
/// otherwise the first registered model. Returns `None` to signal curated fallback.
pub fn default_model_from_manifest(provider: &str) -> Option<String> {
    let models = models_from_manifest(provider)?;
    if let Some((id, _)) = models
        .iter()
        .find(|(id, _)| id == DEFAULT_PROTOCOL_MODEL_ID)
    {
        return Some(id.clone());
    }
    models.into_iter().next().map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_from_manifest_without_protocol_root_returns_none_or_list() {
        // Environment-dependent: either None (no root) or a non-empty list.
        if let Some(models) = models_from_manifest("openai") {
            assert!(!models.is_empty());
        }
    }

    #[test]
    fn default_model_from_manifest_consistent_with_models_list() {
        if let Some(default) = default_model_from_manifest("openai") {
            let models = models_from_manifest("openai").expect("models when default exists");
            assert!(models.iter().any(|(id, _)| id == &default));
        }
    }
}
