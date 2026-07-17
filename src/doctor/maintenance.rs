//! Operator self-maintenance guide: config/policy/protocol vs binary rebuild.

const FOOTER_NO_REBUILD: &str = "Provider/model, agent limits, routes, policy YAML, and $AI_PROTOCOL_DIR manifests can change without rebuilding.";
const FOOTER_CARGO_NOTE: &str = "`cargo build` / `cargo install` always contacts crates.io (or your mirror); that is separate from editing config.";
const FOOTER_FULL_GUIDE: &str =
    "Full guide: `velaclaw doctor maintenance`  (docs/config-externalization.md)";

/// Brief hints printed after the default `velaclaw doctor` summary.
pub fn print_maintenance_footer() {
    println!();
    println!("  [maintenance]");
    println!("    ℹ️  {FOOTER_NO_REBUILD}");
    println!("    ℹ️  {FOOTER_CARGO_NOTE}");
    println!("    📖 {FOOTER_FULL_GUIDE}");
}

/// Full operator guide for `velaclaw doctor maintenance`.
pub fn print_maintenance_guide() {
    println!("📖 VelaClaw operator maintenance guide");
    println!();
    println!("Layers (outside → inside):");
    println!("  L3 Protocol  — $AI_PROTOCOL_DIR manifests (providers, endpoints, models)");
    println!("  L1 Config    — ~/.velaclaw/config.toml (or workspace config)");
    println!("  L2 Policy    — agent-policy.yaml");
    println!("  L2.5 Overrides — <workspace>/.velaclaw/policy-overrides.yaml");
    println!();
    println!("No rebuild needed for:");
    println!("  • Editing config.toml (provider, model, temperature, reliability, model_routes, query_classification)");
    println!("  • Env overrides: VELACLAW_PROVIDER, VELACLAW_MODEL, VELACLAW_TEMPERATURE");
    println!("  • CLI overrides: velaclaw agent -p <provider> --model <model> -m \"…\"");
    println!("  • Policy YAML and workspace policy overrides");
    println!("  • Updating ai-protocol manifests under $AI_PROTOCOL_DIR");
    println!();
    println!("Hot-reload on channel messages (velaclaw channel start, no process restart):");
    println!("  default_provider, default_model, default_temperature, api_key, api_url,");
    println!("  reliability.*, [agent].max_tool_iterations");
    println!();
    println!("Restart process (not rebuild) when changing:");
    println!(
        "  routing.provider_mode, most channel credentials/enable flags, compile-time features"
    );
    println!();
    println!("Preflight after config or protocol changes:");
    println!("  export AI_PROTOCOL_DIR=/path/to/ai-protocol");
    println!("  velaclaw doctor");
    println!("  velaclaw models protocol-providers");
    println!();
    println!("Rebuild / reinstall binary when:");
    println!("  • Runtime code changes, security policy, or new optional channel/tool crates");
    println!("  • Bumping pinned ai-lib-rust or enabling Cargo features (e.g. routing_mvp)");
    println!("  • cargo build / cargo install — always resolves crates; use a mirror if crates.io is slow");
    println!();
    println!("Related:");
    println!("  velaclaw doctor models     — live model catalog probes");
    println!("  velaclaw doctor template-dag --fixture <path> — CR-L2 DAG fixture check (no LLM)");
    println!(
        "  velaclaw doctor candidate-dag --candidate <path> — CR-L4 candidate shadow check (no LLM)"
    );
    println!("  velaclaw providers         — list provider IDs and active default");
    println!("  velaclaw status            — current config summary");
    println!("  docs/config-externalization.md — canonical operator contract");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_footer_mentions_no_rebuild_and_full_guide() {
        assert!(FOOTER_NO_REBUILD.contains("without rebuilding"));
        assert!(FOOTER_CARGO_NOTE.contains("crates.io"));
        assert!(FOOTER_FULL_GUIDE.contains("doctor maintenance"));
    }

    #[test]
    fn maintenance_guide_covers_config_and_rebuild_triggers() {
        let guide_source = include_str!("maintenance.rs");
        assert!(guide_source.contains("config.toml"));
        assert!(guide_source.contains("ai-lib-rust"));
        assert!(guide_source.contains("max_tool_iterations"));
        assert!(guide_source.contains("VELACLAW_PROVIDER"));
    }
}
