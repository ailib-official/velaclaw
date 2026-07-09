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

/// True iff `stdout` honors ANSI escapes right now (TTY + no `NO_COLOR`).
#[must_use]
pub fn ansi_enabled() -> bool {
    tty::ansi_enabled()
}
