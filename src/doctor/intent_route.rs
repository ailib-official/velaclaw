//! CR-CAP-003: doctor observe for intent→Tag→index route (assemble-only; no LLM).

use crate::agent::intent_route::{resolve_for_host, IntentRouteHost};
use crate::config::Config;
use anyhow::Result;

/// Print an explainable intent-route decision.
///
/// Independent of `[agent].intent_capability_route` when `force` is true
/// (doctor observe). When `force` is false, respects the config flag.
pub fn run_intent_route(
    config: &Config,
    message: &str,
    hint: Option<&str>,
    rebuild: bool,
    force: bool,
) -> Result<()> {
    let mut host = IntentRouteHost::from_config(config);
    if force {
        host.enabled = true;
    }

    println!("🩺 VelaClaw Doctor — Intent Capability Route (CR-CAP-003)");
    println!(
        "  flag intent_capability_route: {}",
        config.agent.intent_capability_route
    );
    println!("  observe force-on:          {force}");
    println!("  config_dir:                {}", host.config_dir.display());
    println!();

    if !host.enabled {
        println!("ℹ️  Route disabled (default-off). Prior classification/default path applies.");
        println!("   Enable with `[agent].intent_capability_route = true`, or pass `--force`.");
        return Ok(());
    }

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
        rebuild,
    ) {
        Ok(decision) => {
            println!("hint:               {:?}", decision.hint);
            println!("tags:               {:?}", decision.tags);
            println!(
                "candidates:         {} → {} (after constraints)",
                decision.candidates_before, decision.candidates_after
            );
            println!("truncated:          {:?}", decision.truncated);
            println!("selected_model:     {:?}", decision.selected_model);
            println!("fail_closed:        {}", decision.fail_closed);
            println!("reason:             {}", decision.reason);
            Ok(())
        }
        Err(err) => {
            println!("❌ fail-closed: {err}");
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn doctor_intent_route_force_off_when_disabled() {
        let mut config = Config::default();
        config.agent.intent_capability_route = false;
        run_intent_route(&config, "hello", None, false, false).unwrap();
    }

    #[test]
    fn doctor_intent_route_force_coding() {
        let _guard = ENV_LOCK
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
        run_intent_route(&config, "please refactor", Some("coding"), false, true).unwrap();
        std::env::remove_var("AI_PROTOCOL_DIR");
    }
}
