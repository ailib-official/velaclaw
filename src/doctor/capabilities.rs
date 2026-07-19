//! CR-CAP-002: doctor UX for the host-local capability inverted index.

use crate::capability_index::{
    default_cache_path, load_index, load_or_rebuild_for_config, lookup_tag, protocol_tip,
    CAPABILITY_TAGS, TAG_MAPPING_TABLE,
};
use crate::config::Config;
use crate::protocol_registry::resolve_local_protocol_root;
use anyhow::Result;
use std::path::Path;

/// Operator-facing rebuild triggers (CR-HOST-001). No daily timer (YAGNI).
pub const REBUILD_TRIGGERS_HELP: &str = "\
rebuild triggers (host cache only — never writes public ai-protocol manifests):\n\
  • explicit:   `velaclaw doctor capabilities --rebuild` (or intent-route --rebuild)\n\
  • tip change: AI_PROTOCOL_DIR git HEAD != capability-index.json protocol_tip\n\
  • root change: AI_PROTOCOL_DIR path != cached protocol_root\n\
  • missing/corrupt cache → automatic rebuild on next load\n\
  • not implemented: daily/timer rebuild (use --rebuild or tip change)";

/// Print Tag → candidates (rebuild cache when `rebuild` is set).
pub fn run_capabilities(config: &Config, tag: Option<&str>, rebuild: bool) -> Result<()> {
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
    println!("  protocol_root: {}", index.meta.protocol_root);
    println!("  protocol_tip:  {}", index.meta.protocol_tip);
    println!("  live_tip:      {live_tip}");
    println!("  cache_status:  {cache_status}");
    println!("  built_at:      {}", index.meta.built_at_unix);
    println!("  manifests:     {}", index.meta.provider_manifest_count);
    println!("  cache:         {}", cache_path.display());
    println!("  note:          host cache only — not written to public ai-protocol");
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
            println!("Tag `{tag}` → {} candidate(s)", candidates.len());
            if let Some(entry) = TAG_MAPPING_TABLE.iter().find(|e| e.tag == tag) {
                println!(
                    "  mapping: {:?} wire={:?}",
                    entry.relation, entry.wire_capabilities
                );
                println!("  why:     {}", entry.drift_note);
            }
            if candidates.is_empty() {
                println!("  (empty — Tag is known; no local provider/model matched)");
            } else {
                for c in candidates {
                    match &c.logical_model_id {
                        Some(model) => {
                            println!(
                                "  - {model}  ({})  [{}]  src={}",
                                c.reason, c.provider_id, c.source_file
                            );
                        }
                        None => {
                            println!(
                                "  - {}  ({})  src={}",
                                c.provider_id, c.reason, c.source_file
                            );
                        }
                    }
                }
            }
        }
        None => {
            println!("Tags (capability-mapping.md + host mapping table):");
            for t in CAPABILITY_TAGS {
                let n = index.candidates_for(t).map_or(0, <[_]>::len);
                if let Some(entry) = TAG_MAPPING_TABLE.iter().find(|e| e.tag == *t) {
                    println!("  {t:<24} {n:>3} candidate(s)  [{:?}]", entry.relation);
                } else {
                    println!("  {t:<24} {n:>3} candidate(s)");
                }
            }
            println!();
            println!("Hint: velaclaw doctor capabilities --tag <Tag>");
            println!("      velaclaw doctor capabilities --rebuild");
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
        run_capabilities(&config, Some("coding"), false).expect("doctor");
        run_capabilities(&config, None, false).expect("summary");
        std::env::remove_var("AI_PROTOCOL_DIR");
    }

    #[test]
    fn rebuild_triggers_help_mentions_tip_and_explicit() {
        assert!(REBUILD_TRIGGERS_HELP.contains("protocol_tip"));
        assert!(REBUILD_TRIGGERS_HELP.contains("--rebuild"));
        assert!(REBUILD_TRIGGERS_HELP.contains("not implemented: daily"));
    }
}
