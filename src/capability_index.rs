//! Host-local Tag → candidates inverted index (CR-CAP-002).
//!
//! Built from a local `AI_PROTOCOL_DIR` checkout. Never written into public
//! ai-protocol manifests. Answers facts only; routing stays in host strategy
//! (CR-CAP-003).

use crate::protocol_registry::{
    collect_provider_files, load_manifest_value, resolve_local_protocol_root,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Capability Tags from plans `capability-mapping.md` v0.1.1 (must stay aligned
/// with L2/L4 allowlists).
pub const CAPABILITY_TAGS: &[&str] = &[
    "high-reasoning",
    "coding",
    "speed",
    "document_understanding",
    "tool_calling",
    "long_context",
];

/// Host heuristic: models at or above this window declare `long_context`.
pub const LONG_CONTEXT_MIN_TOKENS: u32 = 128_000;

const CACHE_FILE_NAME: &str = "capability-index.json";
const INDEX_SCHEMA_VERSION: &str = "0.1.0";

/// Explicit Tag ↔ legacy wire mapping (host table; mirrors experimental
/// `capability-tag-mapping` fixture with VC drift notes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TagWireRelation {
    /// Distinctive wire feature required (not bare `text`).
    RequiresDistinctive,
    /// Any of the listed wire capabilities.
    RequiresAny,
    /// No wire feature; index stays empty unless a host heuristic applies.
    UnrelatedWire,
    /// Host capacity heuristic over `metadata.models[].context_window`.
    ContextWindowHeuristic,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagMappingEntry {
    pub tag: &'static str,
    pub relation: TagWireRelation,
    pub wire_capabilities: &'static [&'static str],
    pub drift_note: &'static str,
}

/// Host mapping table — single source for rebuild + doctor docs.
pub const TAG_MAPPING_TABLE: &[TagMappingEntry] = &[
    TagMappingEntry {
        tag: "high-reasoning",
        relation: TagWireRelation::RequiresDistinctive,
        wire_capabilities: &["reasoning"],
        drift_note: "Prefer `reasoning` / extended_thinking; do not treat bare `text` as this Tag",
    },
    TagMappingEntry {
        tag: "coding",
        relation: TagWireRelation::RequiresAny,
        wire_capabilities: &["tools"],
        drift_note: "No dedicated coding wire enum; tools+text capable providers qualify via tools",
    },
    TagMappingEntry {
        tag: "speed",
        relation: TagWireRelation::UnrelatedWire,
        wire_capabilities: &[],
        drift_note:
            "Cost/latency policy class — not a wire feature; candidates empty from manifests",
    },
    TagMappingEntry {
        tag: "document_understanding",
        relation: TagWireRelation::RequiresAny,
        wire_capabilities: &["vision"],
        drift_note: "Vision is minimum wire hint; document content blocks are separate",
    },
    TagMappingEntry {
        tag: "tool_calling",
        relation: TagWireRelation::RequiresAny,
        wire_capabilities: &["tools", "parallel_tools", "agentic"],
        drift_note: "Also accepts capabilities.tool_calling.native.supported=true",
    },
    TagMappingEntry {
        tag: "long_context",
        relation: TagWireRelation::ContextWindowHeuristic,
        wire_capabilities: &[],
        drift_note: "Deferred route Tag; host indexes models with context_window >= 128000",
    },
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityCandidate {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_model_id: Option<String>,
    pub reason: String,
    pub source_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityIndexMeta {
    pub schema_version: String,
    pub built_at_unix: u64,
    pub protocol_root: String,
    /// Best-effort git tip of the protocol checkout (`unknown` if unavailable).
    pub protocol_tip: String,
    pub provider_manifest_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityIndex {
    pub meta: CapabilityIndexMeta,
    /// Tag → sorted candidate list.
    pub by_tag: BTreeMap<String, Vec<CapabilityCandidate>>,
}

impl CapabilityIndex {
    #[must_use]
    pub fn candidates_for(&self, tag: &str) -> Option<&[CapabilityCandidate]> {
        self.by_tag.get(tag).map(Vec::as_slice)
    }

    #[must_use]
    pub fn is_known_tag(tag: &str) -> bool {
        CAPABILITY_TAGS.contains(&tag)
    }
}

/// Query-time reachable view: keep declared candidates whose `provider_id` passes
/// `is_reachable`. Does **not** mutate the cached fact index (CR-CAP-004).
#[must_use]
pub fn filter_reachable(
    candidates: &[CapabilityCandidate],
    is_reachable: impl Fn(&str) -> bool,
) -> Vec<&CapabilityCandidate> {
    candidates
        .iter()
        .filter(|c| is_reachable(&c.provider_id))
        .collect()
}

/// Default cache path: `<config_dir>/capability-index.json`.
#[must_use]
pub fn default_cache_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CACHE_FILE_NAME)
}

/// Best-effort protocol tip (short git SHA or `unknown`).
#[must_use]
pub fn protocol_tip(root: &Path) -> String {
    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-parse",
            "--short",
            "HEAD",
        ])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let tip = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if tip.is_empty() {
                "unknown".into()
            } else {
                tip
            }
        }
        _ => "unknown".into(),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn collect_wire_capabilities(raw: &serde_json::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(caps) = raw.get("capabilities") else {
        return out;
    };
    if let Some(arr) = caps.as_array() {
        for v in arr {
            if let Some(s) = v.as_str() {
                out.insert(s.to_string());
            }
        }
        return out;
    }
    let Some(obj) = caps.as_object() else {
        return out;
    };
    for key in ["required", "optional"] {
        if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    out.insert(s.to_string());
                }
            }
        }
    }
    if caps
        .pointer("/feature_flags/extended_thinking")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        out.insert("reasoning".into());
    }
    if caps
        .pointer("/tool_calling/native/supported")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        out.insert("tools".into());
    }
    out
}

fn has_vision_hint(raw: &serde_json::Value, wire: &BTreeSet<String>) -> bool {
    if wire.contains("vision") {
        return true;
    }
    if raw
        .pointer("/multimodal/vision")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return true;
    }
    if let Some(arr) = raw.get("multimodal").and_then(|m| m.as_array()) {
        return arr
            .iter()
            .any(|v| v.as_str() == Some("vision") || v.as_str() == Some("image"));
    }
    false
}

fn provider_id_from_raw(raw: &serde_json::Value, path: &Path) -> String {
    raw.get("id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".into())
}

fn match_provider_tags(
    provider_id: &str,
    path: &Path,
    raw: &serde_json::Value,
    wire: &BTreeSet<String>,
) -> Vec<(String, CapabilityCandidate)> {
    let mut hits = Vec::new();
    let source = path.display().to_string();

    for entry in TAG_MAPPING_TABLE {
        let matched = match entry.relation {
            TagWireRelation::UnrelatedWire | TagWireRelation::ContextWindowHeuristic => false,
            TagWireRelation::RequiresDistinctive => {
                entry.wire_capabilities.iter().any(|w| wire.contains(*w))
            }
            TagWireRelation::RequiresAny => {
                if entry.tag == "document_understanding" {
                    has_vision_hint(raw, wire)
                } else {
                    entry.wire_capabilities.iter().any(|w| wire.contains(*w))
                }
            }
        };
        if !matched {
            continue;
        }
        let reason = format!(
            "wire match ({:?}): {}",
            entry.relation,
            entry.wire_capabilities.join("|")
        );
        hits.push((
            entry.tag.to_string(),
            CapabilityCandidate {
                provider_id: provider_id.to_string(),
                logical_model_id: None,
                reason,
                source_file: source.clone(),
            },
        ));
    }
    hits
}

fn match_long_context_models(
    provider_id: &str,
    path: &Path,
    raw: &serde_json::Value,
) -> Vec<CapabilityCandidate> {
    let mut out = Vec::new();
    let Some(models) = raw
        .pointer("/metadata/models")
        .and_then(serde_json::Value::as_object)
    else {
        return out;
    };
    let source = path.display().to_string();
    for (model_key, meta) in models {
        let Some(window) = meta
            .get("context_window")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
        else {
            continue;
        };
        if window < LONG_CONTEXT_MIN_TOKENS {
            continue;
        }
        let logical_id = if model_key.contains('/') {
            model_key.clone()
        } else {
            format!("{provider_id}/{model_key}")
        };
        out.push(CapabilityCandidate {
            provider_id: provider_id.to_string(),
            logical_model_id: Some(logical_id),
            reason: format!("context_window={window} >= {LONG_CONTEXT_MIN_TOKENS}"),
            source_file: source.clone(),
        });
    }
    out
}

/// Build index from a local protocol root (no cache I/O).
pub fn build_index(protocol_root: &Path) -> Result<CapabilityIndex> {
    let files = collect_provider_files(protocol_root);
    let mut by_tag: BTreeMap<String, Vec<CapabilityCandidate>> = BTreeMap::new();
    for tag in CAPABILITY_TAGS {
        by_tag.insert((*tag).to_string(), Vec::new());
    }

    for path in &files {
        let Ok(raw) = load_manifest_value(path) else {
            continue;
        };
        let provider_id = provider_id_from_raw(&raw, path);
        let wire = collect_wire_capabilities(&raw);
        for (tag, cand) in match_provider_tags(&provider_id, path, &raw, &wire) {
            by_tag.entry(tag).or_default().push(cand);
        }
        for cand in match_long_context_models(&provider_id, path, &raw) {
            by_tag.entry("long_context".into()).or_default().push(cand);
        }
    }

    for list in by_tag.values_mut() {
        list.sort_by(|a, b| {
            (&a.provider_id, &a.logical_model_id).cmp(&(&b.provider_id, &b.logical_model_id))
        });
        list.dedup_by(|a, b| {
            a.provider_id == b.provider_id && a.logical_model_id == b.logical_model_id
        });
    }

    Ok(CapabilityIndex {
        meta: CapabilityIndexMeta {
            schema_version: INDEX_SCHEMA_VERSION.into(),
            built_at_unix: now_unix(),
            protocol_root: protocol_root.display().to_string(),
            protocol_tip: protocol_tip(protocol_root),
            provider_manifest_count: files.len(),
        },
        by_tag,
    })
}

/// Write index JSON atomically.
pub fn save_index(path: &Path, index: &CapabilityIndex) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create capability-index parent {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(index).context("serialize capability-index")?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &bytes).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Load a previously saved index.
pub fn load_index(path: &Path) -> Result<CapabilityIndex> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let index: CapabilityIndex =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    Ok(index)
}

fn cache_is_fresh(cached: &CapabilityIndex, protocol_root: &Path) -> bool {
    if cached.meta.protocol_root != protocol_root.display().to_string() {
        return false;
    }
    let tip = protocol_tip(protocol_root);
    if tip != "unknown" && cached.meta.protocol_tip != tip {
        return false;
    }
    true
}

/// Load cache if fresh for `protocol_root`; otherwise rebuild and save.
pub fn load_or_rebuild(
    cache_path: &Path,
    protocol_root: &Path,
    force: bool,
) -> Result<CapabilityIndex> {
    if !force {
        if let Ok(cached) = load_index(cache_path) {
            if cache_is_fresh(&cached, protocol_root) {
                return Ok(cached);
            }
        }
    }
    let index = build_index(protocol_root)?;
    save_index(cache_path, &index)?;
    Ok(index)
}

/// Resolve protocol root + cache path from config dir; rebuild as needed.
pub fn load_or_rebuild_for_config(
    config_dir: &Path,
    force: bool,
) -> Result<(CapabilityIndex, PathBuf)> {
    let Some(root) = resolve_local_protocol_root() else {
        bail!(
            "Set AI_PROTOCOL_DIR to a local ai-protocol checkout (not a URL) to build the capability index."
        );
    };
    let cache_path = default_cache_path(config_dir);
    let index = load_or_rebuild(&cache_path, &root, force)?;
    Ok((index, cache_path))
}

/// Lookup candidates for a Tag. Unknown Tag → error. Known but empty → Ok([]).
pub fn lookup_tag<'a>(index: &'a CapabilityIndex, tag: &str) -> Result<&'a [CapabilityCandidate]> {
    if !CapabilityIndex::is_known_tag(tag) {
        bail!(
            "unknown capability Tag '{tag}'; allowed: {}",
            CAPABILITY_TAGS.join(", ")
        );
    }
    Ok(index.candidates_for(tag).unwrap_or(&[]))
}

/// Shared lock for tests that mutate `AI_PROTOCOL_DIR` / `AI_PROTOCOL_PATH`.
#[cfg(test)]
pub(crate) static PROTOCOL_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        old: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let old = std::env::var(key).ok();
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.old.as_ref() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn write_provider(dir: &Path, name: &str, body: &str) {
        let providers = dir.join("v2").join("providers");
        fs::create_dir_all(&providers).expect("providers dir");
        fs::write(providers.join(name), body).expect("write manifest");
    }

    #[test]
    fn filter_reachable_keeps_only_keyed_providers() {
        let candidates = vec![
            CapabilityCandidate {
                provider_id: "groq".into(),
                logical_model_id: None,
                reason: "test".into(),
                source_file: "a".into(),
            },
            CapabilityCandidate {
                provider_id: "anthropic".into(),
                logical_model_id: None,
                reason: "test".into(),
                source_file: "b".into(),
            },
            CapabilityCandidate {
                provider_id: "ollama".into(),
                logical_model_id: Some("ollama/llama3.2".into()),
                reason: "test".into(),
                source_file: "c".into(),
            },
        ];
        let reachable = filter_reachable(&candidates, |id| id == "groq" || id == "ollama");
        assert_eq!(reachable.len(), 2);
        assert_eq!(reachable[0].provider_id, "groq");
        assert_eq!(reachable[1].provider_id, "ollama");
        // Subset of declared — never invents candidates.
        assert!(reachable.iter().all(|c| candidates.iter().any(|d| {
            d.provider_id == c.provider_id && d.logical_model_id == c.logical_model_id
        })));
    }

    #[test]
    fn mapping_table_covers_all_tags() {
        let tags: BTreeSet<_> = TAG_MAPPING_TABLE.iter().map(|e| e.tag).collect();
        for t in CAPABILITY_TAGS {
            assert!(tags.contains(t), "missing mapping for {t}");
        }
        assert_eq!(tags.len(), CAPABILITY_TAGS.len());
    }

    #[test]
    fn unknown_tag_errors_empty_known_ok() {
        let index = CapabilityIndex {
            meta: CapabilityIndexMeta {
                schema_version: INDEX_SCHEMA_VERSION.into(),
                built_at_unix: 0,
                protocol_root: "/tmp".into(),
                protocol_tip: "unknown".into(),
                provider_manifest_count: 0,
            },
            by_tag: CAPABILITY_TAGS
                .iter()
                .map(|t| ((*t).to_string(), Vec::new()))
                .collect(),
        };
        assert!(lookup_tag(&index, "nope").is_err());
        let empty = lookup_tag(&index, "speed").expect("known");
        assert!(empty.is_empty());
    }

    #[test]
    fn build_indexes_wire_and_long_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_provider(
            dir.path(),
            "alpha.json",
            r#"{
              "id": "alpha",
              "capabilities": {
                "required": ["text", "tools"],
                "optional": ["reasoning", "vision"],
                "feature_flags": {"extended_thinking": true},
                "tool_calling": {"native": {"supported": true}}
              },
              "metadata": {
                "models": {
                  "big": {"context_window": 200000},
                  "small": {"context_window": 8192}
                }
              }
            }"#,
        );
        write_provider(
            dir.path(),
            "beta.json",
            r#"{
              "id": "beta",
              "capabilities": {"required": ["text"], "optional": []},
              "metadata": {"models": {"tiny": {"context_window": 4096}}}
            }"#,
        );

        let index = build_index(dir.path()).expect("build");
        assert_eq!(index.meta.provider_manifest_count, 2);

        let coding = lookup_tag(&index, "coding").unwrap();
        assert_eq!(coding.len(), 1);
        assert_eq!(coding[0].provider_id, "alpha");

        let reasoning = lookup_tag(&index, "high-reasoning").unwrap();
        assert_eq!(reasoning.len(), 1);
        assert_eq!(reasoning[0].provider_id, "alpha");

        let docs = lookup_tag(&index, "document_understanding").unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].provider_id, "alpha");

        let tools = lookup_tag(&index, "tool_calling").unwrap();
        assert_eq!(tools.len(), 1);

        let speed = lookup_tag(&index, "speed").unwrap();
        assert!(speed.is_empty(), "speed is unrelated_wire");

        let long = lookup_tag(&index, "long_context").unwrap();
        assert_eq!(long.len(), 1);
        assert_eq!(long[0].logical_model_id.as_deref(), Some("alpha/big"));
    }

    #[test]
    fn rebuild_idempotent_and_cache_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_provider(
            dir.path(),
            "gamma.json",
            r#"{"id":"gamma","capabilities":{"required":["tools"],"optional":[]}}"#,
        );
        let a = build_index(dir.path()).expect("a");
        let b = build_index(dir.path()).expect("b");
        assert_eq!(a.by_tag, b.by_tag);
        assert_eq!(a.meta.protocol_root, b.meta.protocol_root);
        assert_eq!(
            a.meta.provider_manifest_count,
            b.meta.provider_manifest_count
        );

        let cache = dir.path().join("cache").join(CACHE_FILE_NAME);
        save_index(&cache, &a).expect("save");
        let loaded = load_index(&cache).expect("load");
        assert_eq!(loaded.by_tag, a.by_tag);

        let _guard = PROTOCOL_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _env = EnvGuard::set("AI_PROTOCOL_DIR", Some(dir.path().to_str().expect("utf8")));
        let cfg = dir.path().join("cfg");
        let (again, path) = load_or_rebuild_for_config(&cfg, false).expect("lor");
        assert_eq!(again.by_tag.get("coding").map(|v| v.len()), Some(1));
        assert!(path.exists());
    }
}
