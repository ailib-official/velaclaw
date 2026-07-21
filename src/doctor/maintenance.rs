//! Operator self-maintenance guide: config/policy/protocol vs binary rebuild.
//! VL-OPS-001: PATH / install-location hygiene (observe-only).

use std::path::{Path, PathBuf};

const FOOTER_NO_REBUILD: &str = "Provider/model, agent limits, routes, policy YAML, and $AI_PROTOCOL_DIR manifests can change without rebuilding.";
const FOOTER_CARGO_NOTE: &str = "`cargo build` / `cargo install` always contacts crates.io (or your mirror); that is separate from editing config.";
const FOOTER_FULL_GUIDE: &str =
    "Full guide: `velaclaw doctor maintenance`  (docs/config-externalization.md)";
const FOOTER_PATH_HINT: &str =
    "PATH binary: `velaclaw doctor maintenance` shows which install is first on PATH (VL-OPS-001).";

/// Observe-only report of which `velaclaw` binary is running / on PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathBinaryReport {
    pub package_version: String,
    pub running_exe: Option<PathBuf>,
    pub first_on_path: Option<PathBuf>,
    pub known_installs: Vec<PathBuf>,
    /// True when PATH's first hit differs from this process exe, or multiple known installs exist.
    pub warn_ambiguous: bool,
}

/// Brief hints printed after the default `velaclaw doctor` summary.
pub fn print_maintenance_footer() {
    println!();
    println!("  [maintenance]");
    println!("    ℹ️  {FOOTER_NO_REBUILD}");
    println!("    ℹ️  {FOOTER_CARGO_NOTE}");
    println!("    ℹ️  {FOOTER_PATH_HINT}");
    println!("    📖 {FOOTER_FULL_GUIDE}");
}

/// Full operator guide for `velaclaw doctor maintenance`.
pub fn print_maintenance_guide() {
    println!("📖 VelaClaw operator maintenance guide");
    println!();
    print_path_binary_section(&diagnose_path_binaries());
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
    println!(
        "  velaclaw doctor capabilities [--tag <Tag>] [--rebuild] — host Tag→candidates + rebuild triggers"
    );
    println!(
        "  velaclaw doctor capability-route --tag <Tag> --force — capability-index select observe (CR-CAP-007)"
    );
    println!(
        "  velaclaw doctor routing     — explain provider_mode + BYOK effective model (VL-DR-001)"
    );
    println!("  velaclaw providers         — list provider IDs and active default");
    println!("  velaclaw status            — current config summary");
    println!("  docs/config-externalization.md — canonical operator contract");
    println!("  docs/troubleshooting.md — PATH / stale binary tips (VL-OPS-001)");
}

/// Diagnose which `velaclaw` is running and which known install paths exist.
pub fn diagnose_path_binaries() -> PathBinaryReport {
    let running_exe = std::env::current_exe()
        .ok()
        .and_then(|p| canonicalize_soft(&p));
    let first_on_path = which::which("velaclaw")
        .ok()
        .and_then(|p| canonicalize_soft(&p));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let known_installs = existing_known_installs(home.as_deref());
    let warn_ambiguous = ambiguous_installs(
        running_exe.as_deref(),
        first_on_path.as_deref(),
        &known_installs,
    );
    PathBinaryReport {
        package_version: env!("CARGO_PKG_VERSION").to_string(),
        running_exe,
        first_on_path,
        known_installs,
        warn_ambiguous,
    }
}

fn print_path_binary_section(report: &PathBinaryReport) {
    println!("Install / PATH (VL-OPS-001; observe-only — does not rewrite PATH):");
    println!("  package_version:  {}", report.package_version);
    match &report.running_exe {
        Some(p) => println!("  this_process:     {}", p.display()),
        None => println!("  this_process:     (could not resolve current_exe)"),
    }
    match &report.first_on_path {
        Some(p) => println!("  first_on_PATH:    {}", p.display()),
        None => println!("  first_on_PATH:    (velaclaw not found via PATH)"),
    }
    if report.known_installs.is_empty() {
        println!("  known_installs:   (none of ~/bin, ~/.local/bin, /usr/local/bin)");
    } else {
        println!("  known_installs:");
        for p in &report.known_installs {
            println!("    - {}", p.display());
        }
    }
    if report.warn_ambiguous {
        println!("  ⚠️  Multiple or disagreeing installs detected.");
        println!("     Prefer one location on PATH (common: `$HOME/bin` before `~/.local/bin`).");
        println!("     Stale root-owned `~/.local/bin/velaclaw` has caused triage confusion;");
        println!("     compare mtimes or reinstall the PATH-first binary — do not sudo blindly.");
    } else {
        println!("  status:           PATH / this_process look consistent (or single install)");
    }
}

fn existing_known_installs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home {
        for rel in ["bin/velaclaw", ".local/bin/velaclaw"] {
            let p = home.join(rel);
            if p.is_file() {
                out.push(canonicalize_soft(&p).unwrap_or(p));
            }
        }
    }
    let usr = PathBuf::from("/usr/local/bin/velaclaw");
    if usr.is_file() {
        out.push(canonicalize_soft(&usr).unwrap_or(usr));
    }
    out.sort();
    out.dedup();
    out
}

fn ambiguous_installs(
    running: Option<&Path>,
    first_on_path: Option<&Path>,
    known: &[PathBuf],
) -> bool {
    if known.len() > 1 {
        return true;
    }
    match (running, first_on_path) {
        (Some(a), Some(b)) if a != b => true,
        _ => false,
    }
}

fn canonicalize_soft(path: &Path) -> Option<PathBuf> {
    Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn maintenance_footer_mentions_no_rebuild_and_full_guide() {
        assert!(FOOTER_NO_REBUILD.contains("without rebuilding"));
        assert!(FOOTER_CARGO_NOTE.contains("crates.io"));
        assert!(FOOTER_FULL_GUIDE.contains("doctor maintenance"));
        assert!(FOOTER_PATH_HINT.contains("PATH"));
    }

    #[test]
    fn maintenance_guide_covers_config_and_rebuild_triggers() {
        let guide_source = include_str!("maintenance.rs");
        assert!(guide_source.contains("config.toml"));
        assert!(guide_source.contains("ai-lib-rust"));
        assert!(guide_source.contains("max_tool_iterations"));
        assert!(guide_source.contains("VELACLAW_PROVIDER"));
        assert!(guide_source.contains("VL-OPS-001"));
        assert!(guide_source.contains("capability-route"));
    }

    #[test]
    fn ambiguous_when_running_differs_from_path_first() {
        assert!(ambiguous_installs(
            Some(Path::new("/home/user/bin/velaclaw")),
            Some(Path::new("/home/user/.local/bin/velaclaw")),
            &[]
        ));
    }

    #[test]
    fn ambiguous_when_multiple_known_installs() {
        let known = vec![
            PathBuf::from("/home/user/bin/velaclaw"),
            PathBuf::from("/home/user/.local/bin/velaclaw"),
        ];
        assert!(ambiguous_installs(None, None, &known));
    }

    #[test]
    fn not_ambiguous_when_single_matching_path() {
        let p = Path::new("/home/user/bin/velaclaw");
        assert!(!ambiguous_installs(Some(p), Some(p), &[p.to_path_buf()]));
    }

    #[test]
    fn existing_known_installs_finds_home_bin() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("velaclaw");
        fs::write(&exe, b"stub\n").unwrap();
        let found = existing_known_installs(Some(dir.path()));
        assert!(
            found.iter().any(|p| p.ends_with("bin/velaclaw")),
            "expected home bin, got {found:?}"
        );
    }
}
