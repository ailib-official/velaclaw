//! CR-CAP-002: doctor UX for the host-local capability inverted index.

use crate::capability_index::{
    load_or_rebuild_for_config, lookup_tag, CAPABILITY_TAGS, TAG_MAPPING_TABLE,
};
use crate::config::Config;
use anyhow::Result;
use std::path::Path;

/// Print Tag → candidates (rebuild cache when `rebuild` is set).
pub fn run_capabilities(config: &Config, tag: Option<&str>, rebuild: bool) -> Result<()> {
    let config_dir = config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let (index, cache_path) = load_or_rebuild_for_config(config_dir, rebuild)?;

    println!("🩺 VelaClaw Doctor — Capability Index (host-local)");
    println!("  protocol_root: {}", index.meta.protocol_root);
    println!("  protocol_tip:  {}", index.meta.protocol_tip);
    println!("  built_at:      {}", index.meta.built_at_unix);
    println!("  manifests:     {}", index.meta.provider_manifest_count);
    println!("  cache:         {}", cache_path.display());
    println!("  note:          host cache only — not written to public ai-protocol");
    println!();

    if rebuild {
        println!("♻️  Rebuilt capability-index.json");
        println!();
    }

    match tag {
        Some(tag) => {
            let candidates = lookup_tag(&index, tag)?;
            println!("Tag `{tag}` → {} candidate(s)", candidates.len());
            if candidates.is_empty() {
                println!("  (empty — Tag is known; no local provider/model matched)");
                if let Some(entry) = TAG_MAPPING_TABLE.iter().find(|e| e.tag == tag) {
                    println!("  mapping: {:?} — {}", entry.relation, entry.drift_note);
                }
            } else {
                for c in candidates {
                    match &c.logical_model_id {
                        Some(model) => println!("  - {model}  ({})  [{}]", c.reason, c.provider_id),
                        None => println!("  - {}  ({})", c.provider_id, c.reason),
                    }
                }
            }
        }
        None => {
            println!("Tags (from capability-mapping.md):");
            for t in CAPABILITY_TAGS {
                let n = index.candidates_for(t).map_or(0, <[_]>::len);
                println!("  {t:<24} {n} candidate(s)");
            }
            println!();
            println!("Hint: velaclaw doctor capabilities --tag <Tag>");
            println!("      velaclaw doctor capabilities --rebuild");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_index::{build_index, save_index};
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn doctor_capabilities_lists_summary() {
        let _guard = ENV_LOCK
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
}
