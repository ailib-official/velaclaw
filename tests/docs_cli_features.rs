//! VL-DOC-003: Cargo `[features]` and clap docs must not advertise deleted flags.

use std::fs;
use std::path::Path;

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn cargo_feature_names(toml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_features = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_features = t == "[features]";
            continue;
        }
        if !in_features || t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = t.split_once('=') {
            names.push(name.trim().to_string());
        }
    }
    names
}

#[test]
fn default_features_match_cargo_toml() {
    let toml = read("Cargo.toml");
    assert!(
        toml.contains("default = [\"ai-protocol\", \"sandbox-landlock\"]"),
        "Cargo.toml default features drifted"
    );
    let names = cargo_feature_names(&toml);
    for required in [
        "ai-protocol",
        "sandbox-landlock",
        "prism-router",
        "runtime-wasm",
        "remote-deploy",
        "hardware",
    ] {
        assert!(
            names.iter().any(|n| n == required),
            "missing Cargo feature {required:?} in {names:?}"
        );
    }
    assert!(
        !names
            .iter()
            .any(|n| n == "smart-routing" || n == "multi-model" || n == "wasm"),
        "deleted feature names must not return to Cargo.toml: {names:?}"
    );
}

#[test]
fn readme_feature_tables_match_cargo() {
    let en = read("README.md");
    let zh = read("README.zh-CN.md");
    for body in [&en, &zh] {
        assert!(
            body.contains("`sandbox-landlock`"),
            "README must list default sandbox-landlock"
        );
        assert!(
            body.contains("`runtime-wasm`"),
            "README must list optional runtime-wasm"
        );
        assert!(
            !body.contains("--features smart-routing"),
            "deleted Cargo feature smart-routing"
        );
        assert!(
            !body.contains("--features multi-model"),
            "deleted Cargo feature multi-model"
        );
    }
    assert!(
        en.contains("There is no `smart-routing` or `multi-model` Cargo feature"),
        "EN README should state deleted feature names"
    );
}

#[test]
fn operator_docs_mark_removed_clap_flags() {
    let cmd = read("docs/commands-reference.md");
    assert!(
        cmd.contains("## Removed flags"),
        "commands-reference must list removed clap flags"
    );
    let live_agent = Path::new("src/main.rs");
    let main = fs::read_to_string(live_agent).expect("src/main.rs");
    assert!(
        !main.contains("arg(long)]\n        smart") && !main.contains("name = \"smart\""),
        "unexpected --smart in clap (heuristic)"
    );
    assert!(
        !main.contains("name = \"negotiate\"") && !main.contains("long = \"negotiate\""),
        "unexpected --negotiate in clap"
    );
}

#[test]
fn integration_guides_do_not_build_deleted_features() {
    for path in [
        "docs/ai-protocol-integration-guide.md",
        "docs/ai-protocol-integration-guide.zh-CN.md",
    ] {
        let body = read(path);
        assert!(
            !body.contains("--features smart-routing") && !body.contains("`smart-routing`"),
            "{path} still names Cargo feature smart-routing"
        );
        assert!(
            !body.contains("--features multi-model") && !body.contains("`multi-model`"),
            "{path} still names Cargo feature multi-model"
        );
    }
}
