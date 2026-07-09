//! VL-CLI-RENDER-004: end-to-end CLI render integration checks.
//!
//! These tests exercise the public render API the way the CLI sites do
//! (Markdown prompt text + long tool output fold), without a live provider.

use velaclaw::cli_render::{fold, RenderOpts, RenderStyle};
use velaclaw::config::CliRenderConfig;

#[test]
fn cli_render_e2e_markdown_prompt() {
    let opts = RenderOpts {
        style: RenderStyle {
            ansi: false,
            markdown: true,
        },
        fold_lines: 10,
        fold_enabled: false,
    };
    let md = "## Title\n**bold** and `code`\n\n| a | b |\n| - | - |\n| 1 | 2 |";
    let out = opts.render(md);
    assert!(
        !out.contains("## "),
        "heading markers should be rendered away"
    );
    assert!(!out.contains("**"), "bold markers should be rendered away");
    assert!(out.contains("Title"));
    assert!(out.contains("bold"));
    assert!(!out.contains('\u{1b}'), "ansi=false must not emit escapes");
}

#[test]
fn cli_render_e2e_long_tool_output_folded() {
    let body = (1..=20)
        .map(|i| format!("tool-line-{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let folded = fold(&body, 10, Some(1));
    assert_eq!(folded.id, Some(1));
    assert!(folded.visible.contains("/expand 1"));
    assert!(folded.visible.contains("前 10 行 / 共 20 行"));
    assert_eq!(folded.payload, Some(body.as_str()));
    // Expand path: replay payload as-is (no re-render).
    assert_eq!(folded.payload.unwrap(), body);
}

#[test]
fn cli_render_e2e_no_color_flag() {
    let opts = RenderOpts::from_config(
        Some(&CliRenderConfig {
            fold_lines: 10,
            markdown_enabled: true,
        }),
        true,  // --no-color
        false, // --no-fold
        true,  // interactive
    );
    assert!(!opts.style.ansi);
    assert!(!opts.style.markdown);
    let out = opts.render("## H\n**x**");
    assert!(!out.contains('\u{1b}'));
}
