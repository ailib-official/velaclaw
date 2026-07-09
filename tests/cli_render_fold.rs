//! VL-CLI-RENDER-003: long-output fold + `/expand` + config defaults.
//!
//! These tests exercise the pure fold helpers and config wiring without a live
//! provider. Full REPL I/O is covered by unit tests on `print` path helpers and
//! `RenderOpts::from_config`.

use velaclaw::cli_render::{fold, RenderOpts, RenderStyle};
use velaclaw::config::{CliRenderConfig, Config};

#[test]
fn fold_collapses_at_fold_lines_threshold() {
    let text = (1..=12)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let folded = fold(&text, 10, Some(1));
    assert_eq!(folded.id, Some(1));
    assert!(folded.visible.contains("/expand 1"));
    assert!(folded.visible.contains("前 10 行 / 共 12 行"));
    assert_eq!(folded.payload, Some(text.as_str()));
}

#[test]
fn fold_skips_when_below_threshold() {
    let text = "a\nb\nc";
    let folded = fold(text, 10, Some(1));
    assert_eq!(folded.id, None);
    assert_eq!(folded.visible, text);
}

#[test]
fn expand_payload_stored_and_replayable() {
    let text = "x\n".repeat(20);
    let trimmed = text.trim_end_matches('\n');
    let folded = fold(trimmed, 5, Some(42));
    assert_eq!(folded.payload, Some(trimmed));
    assert!(folded.visible.contains("/expand 42"));
}

#[test]
fn no_color_flag_yields_plain_render() {
    let opts = RenderOpts::from_config(None, true, false, true);
    assert!(!opts.style.ansi);
    assert!(!opts.style.markdown);
    let out = opts.render("**bold**");
    assert!(!out.contains('\u{1b}'));
}

#[test]
fn no_fold_flag_yields_full_render() {
    let opts = RenderOpts::from_config(None, false, true, true);
    assert!(!opts.fold_enabled);
}

#[test]
fn config_cli_render_controls_threshold() {
    let cfg = CliRenderConfig {
        fold_lines: 5,
        markdown_enabled: true,
    };
    let opts = RenderOpts::from_config(Some(&cfg), false, false, true);
    assert_eq!(opts.fold_lines, 5);
    assert!(opts.fold_enabled);
}

#[test]
fn default_config_back_compat_no_cli_render_property() {
    let minimal = r#"
default_temperature = 0.7
"#;
    let parsed: Config = toml::from_str(minimal).expect("minimal TOML should parse");
    assert!(parsed.cli_render.is_none());
    let resolved = CliRenderConfig::resolve(parsed.cli_render.as_ref());
    assert_eq!(resolved.fold_lines, 10);
    assert!(resolved.markdown_enabled);
}

#[test]
fn embedded_defaults_include_cli_render() {
    let config = Config::default();
    let resolved = CliRenderConfig::resolve(config.cli_render.as_ref());
    assert_eq!(resolved.fold_lines, 10);
    assert!(resolved.markdown_enabled);
}

#[test]
fn config_cli_render_toml_roundtrip() {
    let config = Config {
        cli_render: Some(CliRenderConfig {
            fold_lines: 7,
            markdown_enabled: false,
        }),
        ..Default::default()
    };
    let toml_str = toml::to_string(&config).expect("serialize");
    let parsed: Config = toml::from_str(&toml_str).expect("deserialize");
    let cr = parsed.cli_render.expect("cli_render present");
    assert_eq!(cr.fold_lines, 7);
    assert!(!cr.markdown_enabled);
}

#[test]
fn markdown_disabled_drops_fences_keeps_inline_markers() {
    // v1 plain mode drops fenced blocks only; inline markers stay (no ANSI).
    let opts = RenderOpts {
        style: RenderStyle {
            ansi: false,
            markdown: false,
        },
        fold_lines: 10,
        fold_enabled: false,
    };
    let out = opts.render("```rust\nfn main() {}\n```\n**bold**");
    assert!(!out.contains("```"));
    assert!(!out.contains("fn main"));
    assert!(out.contains("**bold**"));
    assert!(!out.contains('\u{1b}'));
}
