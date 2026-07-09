//! CLI render layer — pure-function, no IO, no trait coupling.
//!
//! CLI 渲染层 — 纯函数模块：将 Markdown 文字转为终端友好的 ANSI/box 字符串。
//! `src/channels::CliChannel` 与 `src/agent::loop_` 已有的 println/print 互不耦合渲染层。
//!
//! 用法:
//! ```
//! use velaclaw::cli_render::{render, RenderStyle};
//! let s = render("## hi\n**bold**", RenderStyle::auto_markdown());
//! ```

pub mod collapse;
pub mod markdown;
pub mod tty;
pub mod width;

pub use collapse::fold;
pub use markdown::{render, RenderStyle};

use crate::config::CliRenderConfig;

/// Runtime knobs for CLI output: style + fold threshold + whether folding is active.
///
/// Built once per `velaclaw agent` invocation from config + CLI flags + TTY state.
#[derive(Debug, Clone, Copy)]
pub struct RenderOpts {
    pub style: RenderStyle,
    /// Lines kept visible when folding; ignored when `fold_enabled` is false.
    pub fold_lines: usize,
    /// When false, never fold (one-shot, pipe, `--no-fold`).
    pub fold_enabled: bool,
}

impl RenderOpts {
    /// Defaults for interactive TTY: Markdown on, ANSI from TTY/`NO_COLOR`, fold at 10.
    #[must_use]
    pub fn interactive_default() -> Self {
        Self {
            style: RenderStyle::auto_markdown(),
            fold_lines: 10,
            fold_enabled: true,
        }
    }

    /// Build opts from optional `[cli_render]` plus CLI flag overrides.
    ///
    /// - `no_color`: force plain (no ANSI, no Markdown structure)
    /// - `no_fold`: disable long-output folding
    /// - `interactive`: folding only applies in interactive REPL
    #[must_use]
    pub fn from_config(
        cli_render: Option<&CliRenderConfig>,
        no_color: bool,
        no_fold: bool,
        interactive: bool,
    ) -> Self {
        let cfg = CliRenderConfig::resolve(cli_render);
        let style = if no_color {
            RenderStyle::plain()
        } else {
            RenderStyle {
                ansi: ansi_enabled(),
                markdown: cfg.markdown_enabled,
            }
        };
        Self {
            style,
            fold_lines: cfg.fold_lines,
            fold_enabled: interactive && !no_fold,
        }
    }

    /// Render Markdown (or plain) without folding.
    #[must_use]
    pub fn render(self, input: &str) -> String {
        render(input, self.style)
    }
}

/// True iff `stdout` honors ANSI escapes right now (TTY + no `NO_COLOR`).
#[must_use]
pub fn ansi_enabled() -> bool {
    tty::ansi_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_opts_from_config_applies_fold_lines() {
        let cfg = CliRenderConfig {
            fold_lines: 5,
            markdown_enabled: true,
        };
        let opts = RenderOpts::from_config(Some(&cfg), false, false, true);
        assert_eq!(opts.fold_lines, 5);
        assert!(opts.fold_enabled);
        assert!(opts.style.markdown);
    }

    #[test]
    fn render_opts_auto_disables_ansi_when_notty_or_no_color_flag() {
        let opts = RenderOpts::from_config(None, true, false, true);
        assert!(!opts.style.ansi);
        assert!(!opts.style.markdown);
    }

    #[test]
    fn render_opts_respects_no_fold_and_non_interactive() {
        let opts = RenderOpts::from_config(None, false, true, true);
        assert!(!opts.fold_enabled);
        let one_shot = RenderOpts::from_config(None, false, false, false);
        assert!(!one_shot.fold_enabled);
    }

    #[test]
    fn render_opts_markdown_disabled_from_config() {
        let cfg = CliRenderConfig {
            fold_lines: 10,
            markdown_enabled: false,
        };
        let opts = RenderOpts::from_config(Some(&cfg), false, false, true);
        assert!(!opts.style.markdown);
    }
}
