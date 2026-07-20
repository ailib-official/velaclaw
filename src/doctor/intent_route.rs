//! CR-CAP-003/005: doctor observe for capability-index route (assemble-only; no LLM).

use crate::agent::intent_route::{
    hint_to_tag, resolve_for_host, IntentRouteDecision, IntentRouteHost,
};
use crate::capability_index::TAG_MAPPING_TABLE;
use crate::config::Config;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Print an explainable capability-index route decision.
///
/// Independent of `[agent].intent_capability_route` when `force` is true
/// (doctor observe). When `force` is false, respects the config flag.
/// When `persist` is true, append a JSONL decision record under the config dir
/// (opt-in; never enables live chat routing by itself).
///
/// Prefer `--tag <Tag>` (explicit capability) over NL classification.
pub fn run_intent_route(
    config: &Config,
    message: &str,
    hint: Option<&str>,
    tag: Option<&str>,
    rebuild: bool,
    force: bool,
    persist: bool,
) -> Result<()> {
    let mut host = IntentRouteHost::from_config(config);
    if force {
        host.enabled = true;
    }

    println!("🩺 VelaClaw Doctor — Capability Index Route (CR-CAP-005 / CAP-003 wire)");
    println!(
        "  flag intent_capability_route: {}",
        config.agent.intent_capability_route
    );
    println!("  (alias / narrative:          capability-index route; default-off)");
    println!("  observe force-on:          {force}");
    println!("  persist decision:          {persist}");
    println!("  config_dir:                {}", host.config_dir.display());
    println!();

    if !host.enabled {
        println!("ℹ️  Route disabled (default-off). Prior classification/default path applies.");
        println!(
            "   Enable with `[agent].intent_capability_route = true` (capability-index route),"
        );
        println!("   or pass `--force`. Live chat is unchanged unless the config flag is true.");
        return Ok(());
    }

    print_resolve_steps(message, hint, tag);

    let available_hints: Vec<String> = config.model_routes.iter().map(|r| r.hint.clone()).collect();
    let default_model = config
        .default_model
        .as_deref()
        .unwrap_or(crate::config::DEFAULT_PROTOCOL_MODEL_ID);

    match resolve_for_host(
        &host,
        &config.query_classification,
        &available_hints,
        default_model,
        message,
        hint,
        tag,
        rebuild,
    ) {
        Ok(decision) => {
            print_decision(&decision);
            if persist {
                let path = persist_decision(&host.config_dir, &decision)?;
                println!();
                println!("💾 persisted decision → {}", path.display());
            }
            Ok(())
        }
        Err(err) => {
            println!("❌ fail-closed: {err}");
            if persist {
                let synthetic = IntentRouteDecision {
                    enabled: true,
                    hint: hint.map(str::to_string).or_else(|| tag.map(str::to_string)),
                    tags: tag
                        .or(hint)
                        .and_then(hint_to_tag)
                        .map(|t| vec![t.to_string()])
                        .unwrap_or_default(),
                    candidates_before: 0,
                    reachable_before: 0,
                    candidates_after: 0,
                    truncated: Vec::new(),
                    selected_model: None,
                    reason: err.to_string(),
                    fail_closed: true,
                };
                let path = persist_decision(&host.config_dir, &synthetic)?;
                println!("💾 persisted fail-closed → {}", path.display());
            }
            Err(err)
        }
    }
}

fn print_resolve_steps(message: &str, hint: Option<&str>, tag: Option<&str>) {
    println!("steps:");
    println!("  1. input message: {message:?}");
    if let Some(t) = tag {
        println!("  2. explicit Tag:  {t:?} (primary; skips classifier)");
        if let Some(mapped) = hint_to_tag(t) {
            println!("  3. Tag:             {mapped}");
            if let Some(entry) = TAG_MAPPING_TABLE.iter().find(|e| e.tag == mapped) {
                println!(
                    "  4. Tag mapping:     {:?} wire={:?}",
                    entry.relation, entry.wire_capabilities
                );
                println!("     why:             {}", entry.drift_note);
            }
        }
    } else {
        match hint {
            Some(h) => println!("  2. explicit hint:  {h:?} (skips classifier)"),
            None => println!("  2. hint:            from query_classification (optional only)"),
        }
        if let Some(h) = hint {
            if let Some(mapped) = hint_to_tag(h) {
                println!("  3. hint→Tag:        {h:?} → {mapped}");
                if let Some(entry) = TAG_MAPPING_TABLE.iter().find(|e| e.tag == mapped) {
                    println!(
                        "  4. Tag mapping:     {:?} wire={:?}",
                        entry.relation, entry.wire_capabilities
                    );
                    println!("     why:             {}", entry.drift_note);
                }
            } else {
                println!("  3. hint→Tag:        {h:?} → (none — fail-closed if route enabled)");
            }
        } else {
            println!("  3. hint→Tag:        (after classification, if any)");
        }
    }
    println!("  5. declared → reachable (keys) ∩ [[model_routes]] → selected_model or fail-closed");
    println!();
}

fn print_decision(decision: &IntentRouteDecision) {
    println!("decision:");
    println!("  hint:               {:?}", decision.hint);
    println!("  tags:               {:?}", decision.tags);
    println!(
        "  candidates:         declared {} → reachable {} → after constraints {}",
        decision.candidates_before, decision.reachable_before, decision.candidates_after
    );
    println!("  truncated:          {:?}", decision.truncated);
    println!("  selected_model:     {:?}", decision.selected_model);
    println!("  fail_closed:        {}", decision.fail_closed);
    println!("  reason:             {}", decision.reason);
}

fn persist_decision(
    config_dir: &Path,
    decision: &IntentRouteDecision,
) -> Result<std::path::PathBuf> {
    let path = config_dir.join("intent-route-decisions.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    let payload = serde_json::json!({
        "recorded_at_unix": ts,
        "decision": decision,
    });
    writeln!(file, "{}", serde_json::to_string(&payload)?)
        .with_context(|| format!("append {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_index::PROTOCOL_ENV_LOCK;
    use std::fs;

    #[test]
    fn doctor_intent_route_force_off_when_disabled() {
        let mut config = Config::default();
        config.agent.intent_capability_route = false;
        run_intent_route(&config, "hello", None, None, false, false, false).unwrap();
    }

    #[test]
    fn doctor_intent_route_force_coding() {
        let _guard = PROTOCOL_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let providers = dir.path().join("v2").join("providers");
        fs::create_dir_all(&providers).unwrap();
        fs::write(
            providers.join("demo.json"),
            r#"{"id":"demo","capabilities":{"required":["tools"],"optional":[]}}"#,
        )
        .unwrap();
        std::env::set_var("AI_PROTOCOL_DIR", dir.path());

        let cfg_dir = dir.path().join("cfg");
        fs::create_dir_all(&cfg_dir).unwrap();
        let mut config = Config::default();
        config.config_path = cfg_dir.join("config.toml");
        config.agent.intent_capability_route = false;
        // demo has no API key → reachable may be empty → fail-closed is OK for observe.
        let _ = run_intent_route(
            &config,
            "please refactor",
            None,
            Some("coding"),
            false,
            true,
            true,
        );
        let persisted = cfg_dir.join("intent-route-decisions.jsonl");
        assert!(persisted.is_file(), "persist should write jsonl");
        let body = fs::read_to_string(&persisted).unwrap();
        assert!(body.contains("selected_model") || body.contains("fail_closed"));
        std::env::remove_var("AI_PROTOCOL_DIR");
    }
}
