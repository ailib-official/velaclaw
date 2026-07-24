//! CR-CAP-002/004: doctor UX for the host-local capability inverted index
//! and query-time reachable (keys ∩ declared) view.

use crate::capability_index::{
    default_cache_path, filter_reachable, load_index, load_or_rebuild_for_config, lookup_tag,
    protocol_tip, CAPABILITY_TAGS, TAG_MAPPING_TABLE,
};
use crate::config::Config;
use crate::execution::provider_has_usable_key;
use crate::protocol_registry::resolve_local_protocol_root;
use anyhow::Result;
use std::path::Path;

/// Operator-facing rebuild triggers (CR-HOST-001). No daily timer (YAGNI).
pub const REBUILD_TRIGGERS_HELP: &str = "\
rebuild triggers (host cache only — never writes public ai-protocol manifests):\n\
  • explicit:   `velaclaw doctor capabilities --rebuild` (or capability-route --rebuild)\n\
  • tip change: AI_PROTOCOL_DIR git HEAD != capability-index.json protocol_tip\n\
  • root change: AI_PROTOCOL_DIR path != cached protocol_root\n\
  • missing/corrupt cache → automatic rebuild on next load\n\
  • ME-001: when `metadata.models` has model_capabilities/modalities, Tag facts are per-model (prefer model over provider ads; omit = fall back to ads)\n\
  • not implemented: daily/timer rebuild (use --rebuild or tip change)";

/// Print Tag → candidates (rebuild cache when `rebuild` is set).
///
/// `reachable_only`: when set with `--tag`, list only providers with a usable
/// local key (CR-CAP-004). Summary mode always prints declared vs reachable counts.
pub fn run_capabilities(
    config: &Config,
    tag: Option<&str>,
    rebuild: bool,
    reachable_only: bool,
) -> Result<()> {
    let config_dir = config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let cache_path = default_cache_path(config_dir);
    let live_root = resolve_local_protocol_root();
    let live_tip = live_root
        .as_ref()
        .map(|r| protocol_tip(r))
        .unwrap_or_else(|| "unset".into());

    let cache_status = describe_cache_status(&cache_path, live_root.as_deref(), &live_tip);

    let (index, cache_path) = load_or_rebuild_for_config(config_dir, rebuild)?;

    println!("🩺 VelaClaw Doctor — Capability Index (host-local)");
    println!("  {}", super::cap_pipeline::CAP_PIPELINE_LINE);
    println!("  protocol_root: {}", index.meta.protocol_root);
    println!("  protocol_tip:  {}", index.meta.protocol_tip);
    println!("  live_tip:      {live_tip}");
    println!("  cache_status:  {cache_status}");
    println!("  built_at:      {}", index.meta.built_at_unix);
    println!("  manifests:     {}", index.meta.provider_manifest_count);
    println!("  cache:         {}", cache_path.display());
    println!("  note:          host cache only — not written to public ai-protocol");
    println!("  reachable:     query-time keys ∩ declared (no secrets in cache)");
    println!();
    println!("{REBUILD_TRIGGERS_HELP}");
    println!();

    if rebuild {
        println!("♻️  Rebuilt capability-index.json");
        println!();
    }

    match tag {
        Some(tag) => {
            let candidates = lookup_tag(&index, tag)?;
            let reachable = filter_reachable(candidates, provider_has_usable_key);
            if let Some(entry) = TAG_MAPPING_TABLE.iter().find(|e| e.tag == tag) {
                println!(
                    "  mapping: {:?} wire={:?}",
                    entry.relation, entry.wire_capabilities
                );
                println!("  why:     {}", entry.drift_note);
            }
            println!(
                "Tag `{tag}` → {} declared / {} reachable",
                candidates.len(),
                reachable.len()
            );
            let list = if reachable_only {
                println!("  (showing reachable-only)");
                reachable
            } else {
                candidates.iter().collect()
            };
            if list.is_empty() {
                if reachable_only {
                    println!("  (empty — no keyed/keyless-local provider matched this Tag)");
                } else {
                    println!("  (empty — Tag is known; no local provider/model matched)");
                }
            } else {
                for c in list {
                    let mark = if provider_has_usable_key(&c.provider_id) {
                        "reachable"
                    } else {
                        "no-key"
                    };
                    match &c.logical_model_id {
                        Some(model) => {
                            println!(
                                "  - [{mark}] {model}  ({})  [{}]  src={}",
                                c.reason, c.provider_id, c.source_file
                            );
                        }
                        None => {
                            println!(
                                "  - [{mark}] {}  ({})  src={}",
                                c.provider_id, c.reason, c.source_file
                            );
                        }
                    }
                }
            }
            println!();
            println!("Next: velaclaw doctor capability-route --tag {tag} --force");
        }
        None => {
            println!("Tags (declared = protocol facts; reachable = usable local key):");
            for t in CAPABILITY_TAGS {
                let declared = index.candidates_for(t).map_or(0, <[_]>::len);
                let reachable = index
                    .candidates_for(t)
                    .map(|c| filter_reachable(c, provider_has_usable_key).len())
                    .unwrap_or(0);
                if let Some(entry) = TAG_MAPPING_TABLE.iter().find(|e| e.tag == *t) {
                    println!(
                        "  {t:<24} {declared:>3} declared / {reachable:>3} reachable  [{:?}]",
                        entry.relation
                    );
                } else {
                    println!("  {t:<24} {declared:>3} declared / {reachable:>3} reachable");
                }
            }
            println!();
            println!("Hint: velaclaw doctor capabilities --tag <Tag>");
            println!("      velaclaw doctor capabilities --tag <Tag> --reachable-only");
            println!("      velaclaw doctor capabilities --rebuild");
            println!();
            println!("{}", super::cap_pipeline::CAP_RELATED_DOCTOR);
        }
    }

    Ok(())
}

fn describe_cache_status(
    cache_path: &Path,
    live_root: Option<&Path>,
    live_tip: &str,
) -> &'static str {
    let Some(root) = live_root else {
        return "unavailable (set AI_PROTOCOL_DIR)";
    };
    match load_index(cache_path) {
        Ok(cached) => {
            if cached.meta.protocol_root != root.display().to_string() {
                return "stale (protocol_root mismatch)";
            }
            if live_tip != "unknown" && cached.meta.protocol_tip != live_tip {
                return "stale (protocol_tip mismatch)";
            }
            "fresh"
        }
        Err(_) => "missing_or_corrupt",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_index::{build_index, save_index, PROTOCOL_ENV_LOCK};
    use std::fs;

    #[test]
    fn doctor_capabilities_lists_summary() {
        let _guard = PROTOCOL_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let providers = dir.path().join("v2").join("providers");
        fs::create_dir_all(&providers).unwrap();
        fs::write(
            providers.join("demo.json"),
            r#"{"id":"demo","capabilities":{"required":["tools"],"optional":["vision"]}}"#,
        )
        .unwrap();
        std::env::set_var("AI_PROTOCOL_DIR", dir.path());

        let cfg_dir = dir.path().join("cfg");
        fs::create_dir_all(&cfg_dir).unwrap();
        let index = build_index(dir.path()).unwrap();
        save_index(&cfg_dir.join("capability-index.json"), &index).unwrap();

        let mut config = Config::default();
        config.config_path = cfg_dir.join("config.toml");
        run_capabilities(&config, Some("coding"), false, false).expect("doctor");
        run_capabilities(&config, Some("coding"), false, true).expect("reachable-only");
        run_capabilities(&config, None, false, false).expect("summary");
        std::env::remove_var("AI_PROTOCOL_DIR");
    }

    #[test]
    fn rebuild_triggers_help_mentions_tip_and_explicit() {
        assert!(REBUILD_TRIGGERS_HELP.contains("protocol_tip"));
        assert!(REBUILD_TRIGGERS_HELP.contains("--rebuild"));
        assert!(REBUILD_TRIGGERS_HELP.contains("not implemented: daily"));
    }
}
