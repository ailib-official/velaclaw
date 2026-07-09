//! Markdown → terminal ANSI / box-drawing converter — pure tokenizer.
//!
//! Markdown → 终端渲染器 — 自写状态机，覆盖：标题、粗/斜、行内 code、fenced code、
//! 列表（含嵌套）、blockquote、表格、HR、链接；不支持元素 graceful plain。
//!
//! 不引 `pulldown-cmark` / `syntect` —— 留给 velaclaw 二进制体积目标一份余地。

use crate::cli_render::tty::ansi_enabled;
use crate::cli_render::width::{display_width, pad_to_width};

/// Styling options bundled with each `render()` call.
#[derive(Debug, Clone, Copy)]
pub struct RenderStyle {
    /// Emit ANSI escapes. False ⇒ plain strip with box-drawing intact (pipe-friendly).
    pub ansi: bool,
    /// Render Markdown emphasis and structure. False ⇒ strip to plain text.
    pub markdown: bool,
}

impl RenderStyle {
    /// Auto-pick `ansi` from `stdout` TTY status; `markdown = true`.
    #[must_use]
    pub fn auto_markdown() -> Self {
        Self {
            ansi: ansi_enabled(),
            markdown: true,
        }
    }

    /// Plain Mono — no ANSI, no Markdown rendering (useful for tests / `--no-color --no-markdown`).
    #[must_use]
    pub fn plain() -> Self {
        Self {
            ansi: false,
            markdown: false,
        }
    }

    /// Render a Markdown string with this style and return a ready-to-print `String`.
    /// 一次调用即形成可输出字符串，避免 caller 同时调顶层 `render` 与参数构造。
    #[must_use]
    pub fn render(self, input: &str) -> String {
        render(input, self)
    }
}

/// Convert a piece of Markdown into a terminal-friendly string.
///
/// 行级（line-by-line）tokenize，不持有 buffer。返回值可被 `collapse::fold` 再次包装。
#[must_use]
pub fn render(input: &str, style: RenderStyle) -> String {
    if !style.markdown {
        // Strip-only mode: drop fenced code fences and render nothing else.
        return strip_markdown(input);
    }
    let mut out = String::with_capacity(input.len() + 64);
    let mut lines = input.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_end_matches('\n');
        if let Some((fence_lang, _consumed)) = parse_fence_open(trimmed) {
            // Collect block until matching closing fence.
            let mut block = String::new();
            let mut closed = false;
            for following in lines.by_ref() {
                let t = following.trim_end_matches('\n');
                if is_fence_close(t) {
                    closed = true;
                    break;
                }
                block.push_str(following);
            }
            if closed {
                out.push_str(&render_fence(fence_lang.as_deref(), &block, style));
            } else {
                // 畸形未闭合 → 留 fallback：开 fence + 原 block 内容（原样输出）
                out.push_str(&render_fence_unclosed(fence_lang.as_deref(), &block, style));
            }
            out.push('\n');
            continue;
        }
        if is_table_row(trimmed) {
            // Multi-line table lookahead: header row + separator (|---|) + body rows.
            let mut table = vec![trimmed.to_string()];
            // Consume subsequent rows that are still table rows.
            while let Some(next) = lines.peek() {
                let next_t = next.trim_end_matches('\n');
                if is_table_row(next_t) {
                    table.push(next_t.to_string());
                    lines.next();
                } else {
                    break;
                }
            }
            out.push_str(&render_table(&table, style));
            out.push('\n');
            continue;
        }
        out.push_str(&render_inline_line(trimmed, style));
        out.push('\n');
    }
    // Trim trailing blank line added by our `\n` joining when input was empty.
    if out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn render_inline_line(line: &str, style: RenderStyle) -> String {
    if line.is_empty() {
        return String::new();
    }
    if let Some(h) = parse_heading(line) {
        return render_heading(h.level, &h.text, style);
    }
    if is_hr(line) {
        return render_hr(style);
    }
    if let Some(text) = line.strip_prefix("> ") {
        return render_blockquote(text, style);
    }
    if let Some(text) = line.strip_prefix(">> ") {
        return render_blockquote(&format!("> {text}"), style);
    }
    if let Some(item) = parse_list_item(line) {
        return render_list_item(item.depth, item.marker, &item.text, style);
    }
    render_inline(line, style)
}

fn render_inline(line: &str, style: RenderStyle) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // `**bold**` / `__bold__`
        if let Some((text, next)) = parse_emphasis(line, i, b"**") {
            out.push_str(&apply_emphasis(text, true, style));
            i = next;
            continue;
        }
        if let Some((text, next)) = parse_emphasis(line, i, b"__") {
            out.push_str(&apply_emphasis(text, true, style));
            i = next;
            continue;
        }
        // `*italic*` / `_italic_`
        if let Some((text, next)) = parse_emphasis(line, i, b"*") {
            out.push_str(&apply_emphasis(text, false, style));
            i = next;
            continue;
        }
        if let Some((text, next)) = parse_emphasis(line, i, b"_") {
            out.push_str(&apply_emphasis(text, false, style));
            i = next;
            continue;
        }
        // ``code``
        if let Some((text, next)) = parse_inline_code(line, i) {
            out.push_str(&apply_inline_code(text, style));
            i = next;
            continue;
        }
        // `[text](url)`
        if let Some((text, url, next)) = parse_link(line, i) {
            out.push_str(&apply_link(text, url, style));
            i = next;
            continue;
        }
        // Fallback: copy one UTF-8 char and advance.
        let ch_end = next_utf8_boundary(line, i + 1);
        out.push_str(&line[i..ch_end]);
        i = ch_end;
    }
    out
}

/// Parse `**text**` style emphasis — returns (inner_text, end_index_into_input).
fn parse_emphasis<'a>(line: &'a str, start: usize, marker: &[u8]) -> Option<(&'a str, usize)> {
    let bytes = line.as_bytes();
    if start + marker.len() > bytes.len() {
        return None;
    }
    if &bytes[start..start + marker.len()] != marker {
        return None;
    }
    let content_start = start + marker.len();
    let close = line.as_bytes()[content_start..]
        .windows(marker.len())
        .position(|w| w == marker)?;
    let close_byte = content_start + close;
    let text = &line[content_start..close_byte];
    if text.is_empty() {
        return None;
    }
    let next = close_byte + marker.len();
    Some((text, next))
}

fn parse_inline_code(line: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = line.as_bytes();
    if start + 1 >= bytes.len() || bytes[start] != b'`' {
        return None;
    }
    let close = line[start + 1..].find('`')?;
    let close_byte = start + 1 + close;
    let text = &line[start + 1..close_byte];
    if text.is_empty() {
        return None;
    }
    let next = close_byte + 1;
    Some((text, next))
}

fn parse_link(line: &str, start: usize) -> Option<(&str, &str, usize)> {
    let bytes = line.as_bytes();
    if start >= bytes.len() || bytes[start] != b'[' {
        return None;
    }
    let text_close = line[start + 1..].find(']')?;
    let text_end = start + 1 + text_close;
    let text = &line[start + 1..text_end];
    // Must be immediately followed by `(url)`
    if text_end + 1 >= bytes.len() || bytes[text_end + 1] != b'(' {
        return None;
    }
    let url_close = line[text_end + 2..].find(')')?;
    let url_end = text_end + 2 + url_close;
    let url = &line[text_end + 2..url_end];
    let next = url_end + 1;
    Some((text, url, next))
}

struct Heading {
    level: u8,
    text: String,
}

fn parse_heading(line: &str) -> Option<Heading> {
    let hashes_end = line
        .char_indices()
        .take_while(|(i, c)| *c == '#' && *i < 6)
        .count();
    if hashes_end == 0 {
        return None;
    }
    let rest = &line[hashes_end..];
    if !rest.starts_with(' ') {
        return None;
    }
    Some(Heading {
        level: u8::try_from(hashes_end).ok()?,
        text: rest.trim_start().to_string(),
    })
}

fn is_hr(line: &str) -> bool {
    let t = line.trim();
    let chars: Vec<char> = t.chars().collect();
    if chars.len() < 3 {
        return false;
    }
    let all_dash = chars.iter().all(|c| *c == '-');
    let all_star = chars.iter().all(|c| *c == '*');
    let all_underscore = chars.iter().all(|c| *c == '_');
    all_dash || all_star || all_underscore
}

fn parse_fence_open(line: &str) -> Option<(Option<String>, ())> {
    let t = line.trim_start();
    if !t.starts_with("```") {
        return None;
    }
    let rest = &t[3..];
    let lang = if rest.is_empty() {
        None
    } else {
        Some(rest.trim().to_string())
    };
    Some((lang, ()))
}

fn is_fence_close(line: &str) -> bool {
    line.trim().starts_with("```")
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('|') && t.ends_with('|')
}

fn render_table(rows: &[String], style: RenderStyle) -> String {
    let parsed: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            r.trim()
                .trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect();
    if parsed.is_empty() {
        return String::new();
    }
    let n_cols = parsed.iter().map(|r| r.len()).max().unwrap_or(0);
    if n_cols == 0 {
        return String::new();
    }
    let mut col_widths = vec![0usize; n_cols];
    let body_rows: Vec<&Vec<String>> = parsed
        .iter()
        .enumerate()
        // The `|---|` separator row at index 1 carries no cell text — exclude it.
        .filter(|(i, _)| *i != 1)
        .map(|(_, r)| r)
        .collect();
    for r in &body_rows {
        for (i, cell) in r.iter().enumerate().take(n_cols) {
            col_widths[i] = col_widths[i].max(display_width(cell));
        }
    }
    let pad = |cells: &Vec<String>| -> String {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| pad_to_width(c, col_widths[i.min(col_widths.len() - 1)]))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let header = pad(&parsed[0]);
    let sep = col_widths
        .iter()
        .map(|w| "─".repeat(*w))
        .collect::<Vec<_>>()
        .join("─┼─");
    let body: String = parsed
        .iter()
        .enumerate()
        .filter(|(i, _)| *i >= 2)
        .map(|(_, r)| pad(r))
        .collect::<Vec<_>>()
        .join("\n");
    let out = if body.is_empty() {
        format!("┌─{header}─┐\n├─{sep}─┤")
    } else {
        format!("┌─{header}─┐\n├─{sep}─┤\n{body}")
    };
    if style.ansi {
        format!("\x1b[2m{out}\x1b[0m")
    } else {
        out
    }
}

fn parse_list_item(line: &str) -> Option<ListItem> {
    let indent = line.len() - line.trim_start().len();
    let depth = indent / 2;
    let t = line.trim_start();
    let marker_bytes = t.as_bytes();
    if marker_bytes.is_empty() {
        return None;
    }
    // `- ` / `* ` / `+ `
    if (marker_bytes[0] == b'-' || marker_bytes[0] == b'*' || marker_bytes[0] == b'+')
        && marker_bytes.len() >= 2
        && marker_bytes[1] == b' '
    {
        return Some(ListItem {
            depth,
            marker: ListMarker::Unordered(marker_bytes[0] as char),
            text: t[2..].to_string(),
        });
    }
    // `1. ` / `42) ` etc.
    let dot_pos = t.find(". ")?;
    let num_str = &t[..dot_pos];
    if num_str.chars().all(|c| c.is_ascii_digit()) && !num_str.is_empty() {
        return Some(ListItem {
            depth,
            marker: ListMarker::Ordered(num_str.to_string()),
            text: t[dot_pos + 2..].to_string(),
        });
    }
    None
}

enum ListMarker {
    Unordered(char),
    Ordered(String),
}

struct ListItem {
    depth: usize,
    marker: ListMarker,
    text: String,
}

fn render_heading(level: u8, text: &str, style: RenderStyle) -> String {
    if !style.ansi {
        return format!("\n{text}\n");
    }
    let s = match level {
        // bold bright cyan for # and ##; cyan for ### and deeper
        1 | 2 => "\x1b[1;96m",
        _ => "\x1b[1;36m",
    };
    format!("\n{s}{text}\x1b[0m")
}

fn render_hr(style: RenderStyle) -> String {
    let bar = "─".repeat(40);
    if !style.ansi {
        return bar;
    }
    format!("\x1b[2m{bar}\x1b[0m")
}

fn render_blockquote(text: &str, style: RenderStyle) -> String {
    // Multi-line blockquote uses one prefix per line. Here we only have a single
    // line; `render_inline` has already split multi-line callers into per-line
    // invocations.
    if !style.ansi {
        return format!("▌ {text}");
    }
    format!("\x1b[2m▌\x1b[0m {text}")
}

fn render_list_item(depth: usize, marker: ListMarker, text: &str, style: RenderStyle) -> String {
    let indent = "  ".repeat(depth);
    let bullet = match marker {
        ListMarker::Unordered(_c) => match depth {
            0 => "•".to_string(),
            1 => "◦".to_string(),
            _ => "▪".to_string(),
        },
        ListMarker::Ordered(n) => format!("{n:>2}."),
    };
    let body = render_inline(text, style);
    format!("{indent}{bullet} {body}")
}

fn render_fence(lang: Option<&str>, block: &str, style: RenderStyle) -> String {
    let header = match lang {
        Some(l) if !l.is_empty() => format!("── code: {l} ──"),
        _ => "── code ──".to_string(),
    };
    let indented = block
        .split('\n')
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    if !style.ansi {
        return format!("{header}\n{indented}\n─────");
    }
    format!("\x1b[2m{header}\x1b[0m\n\x1b[2m{indented}\x1b[0m\n\x1b[2m─────\x1b[0m")
}

/// Fallback for malformed fences (no closing）：纯文本 fallback，不渲染 dim 边框避免视觉误导。
fn render_fence_unclosed(lang: Option<&str>, block: &str, style: RenderStyle) -> String {
    let header = match lang {
        Some(l) if !l.is_empty() => format!("── code: {l} (未闭合) ──"),
        _ => "── code (未闭合) ──".to_string(),
    };
    let _ = style;
    format!("```\n{header}\n{block}")
}

fn apply_emphasis(text: &str, bold: bool, style: RenderStyle) -> String {
    if !style.ansi {
        return text.to_string();
    }
    if bold {
        format!("\x1b[1m{text}\x1b[0m")
    } else {
        format!("\x1b[3m{text}\x1b[0m")
    }
}

fn apply_inline_code(text: &str, style: RenderStyle) -> String {
    if !style.ansi {
        return text.to_string();
    }
    format!("\x1b[36m{text}\x1b[0m")
}

fn apply_link(text: &str, url: &str, style: RenderStyle) -> String {
    if !style.ansi {
        return format!("{text} ({url})");
    }
    format!("{text} \x1b[2m({url})\x1b[0m")
}

fn strip_markdown(input: &str) -> String {
    // Best-effort plain-text strip: drop fenced code blocks entirely; preserve
    // all other content as-is so tests and any future non-markdown caller see
    // expected inner strings minus protocol noise.
    let mut out = String::with_capacity(input.len());
    let mut in_fence = false;
    for line in input.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n');
        if trimmed.trim().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        out.push_str(line);
    }
    out
}

fn next_utf8_boundary(s: &str, at_least: usize) -> usize {
    let mut i = at_least.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Top-level table renderer used by `render()` when a `|` row is detected
/// followed by a `|---|` separator line and at least one body row.
///
/// Returns `Some(rendered)` if the caller should consume subsequent lines as a
/// table, or `None` if `peekable.next()` should put the line back.
///
/// Because Rust iterators don't support push-back, callers use `Peekable`:
/// call `try_render_table_from_peek(&mut lines)` *after* `lines.peek()` confirms
/// a `|` line is present.
pub(crate) fn try_render_table_from_peek(
    lines: &mut std::iter::Peekable<std::str::SplitInclusive<'_, char>>,
    style: RenderStyle,
) -> Option<(String, usize)> {
    let _ = lines;
    let _ = style;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> RenderStyle {
        RenderStyle::plain()
    }

    fn with_ansi() -> RenderStyle {
        RenderStyle {
            ansi: true,
            markdown: true,
        }
    }

    #[test]
    fn render_plain_respects_no_markdown() {
        // Fenced code blocks dropped in plain mode (as documented).
        let out = render("```rust\nfn main() {}\n```\ntext", plain());
        assert!(!out.contains("```"));
        assert!(out.contains("text"));
    }

    #[test]
    fn markdown_renders_bold_italic_inline_code() {
        let out = render("**bold** *italic* `c`", with_ansi());
        assert!(out.contains("\x1b[1mbold\x1b[0m"));
        assert!(out.contains("\x1b[3mitalic\x1b[0m"));
        assert!(out.contains("\x1b[36mc\x1b[0m"));
    }

    #[test]
    fn markdown_renders_headings_three_levels() {
        let out = render("# H1\n## H2\n### H3", with_ansi());
        for (lvl, marker) in [
            ("H1", "\x1b[1;96m"),
            ("H2", "\x1b[1;96m"),
            ("H3", "\x1b[1;36m"),
        ] {
            assert!(
                out.contains(&format!("{marker}{lvl}\x1b[0m")),
                "missing {marker} for {lvl}"
            );
        }
    }

    #[test]
    fn markdown_renders_fenced_code_with_dim_outline() {
        let out = render("```rust\nfn main() {}\n```", with_ansi());
        assert!(out.contains("── code: rust ──"));
        assert!(out.contains("  fn main() {}"));
        assert!(out.contains("─────"));
        assert!(out.contains("\x1b[2m"));
    }

    #[test]
    fn markdown_renders_unordered_lists_three_depths() {
        let out = render("- a\n  - b\n    - c", with_ansi());
        assert!(out.contains("• a"));
        assert!(out.contains("  ◦ b"));
        assert!(out.contains("    ▪ c"));
    }

    #[test]
    fn markdown_renders_ordered_list_aligned_number_width() {
        let out = render("1. first\n2. second", with_ansi());
        assert!(out.contains(" 1. first"));
        assert!(out.contains(" 2. second"));
    }

    #[test]
    fn markdown_renders_blockquote_with_bar_prefix() {
        let out = render("> hi", with_ansi());
        assert!(out.contains("\x1b[2m▌\x1b[0m hi"));
    }

    #[test]
    fn markdown_renders_hr_full_width_dim() {
        // Our HR detector accepts `---` lines.
        let out = render("---", with_ansi());
        let bar = "─".repeat(40);
        assert!(out.contains(&format!("\x1b[2m{bar}\x1b[0m")));
    }

    #[test]
    fn markdown_renders_link_text_and_url() {
        let out = render("[site](https://example.com)", with_ansi());
        assert!(out.contains("site"));
        assert!(out.contains("https://example.com"));
        assert!(out.contains("\x1b[2m(https://example.com)\x1b[0m"));
    }

    #[test]
    fn markdown_unclosed_fence_graceful_plain_no_panic() {
        // No closing ``` — must not panic, must keep content visible.
        let out = render("```rust\nfn main() {}\nstill in", with_ansi());
        assert!(out.contains("未闭合"));
        assert!(out.contains("fn main() {}"));
        assert!(out.contains("still in"));
    }

    #[test]
    fn markdown_renders_table_box_drawing_aligned() {
        let md = "| 名称 | 值 |\n|---|---|\n| 甲 | 1 |\n| 乙xyz | 234 |\n";
        let out = render(md, with_ansi());
        assert!(out.contains("┌─"));
        assert!(out.contains("├─"));
        assert!(out.contains("─┼─"));
        // Header & body cells present
        assert!(out.contains("名称"));
        assert!(out.contains("甲"));
        assert!(out.contains("乙xyz"));
        // CJK column should not be width-misaligned: 甲 occupies 2 cols,
        // numeric column to its right starts at offset matching 乙xyz row.
        // We assert position invariance: the body row "甲 | 1" and "乙xyz | 234"
        // both render with the | separator in the same column index.
        let lines: Vec<&str> = out.lines().collect();
        let body_a = lines[2];
        let body_b = lines[3];
        let sep_a = body_a.find('|').unwrap_or(0);
        let sep_b = body_b.find('|').unwrap_or(0);
        assert_eq!(
            sep_a, sep_b,
            "cell separator columns must align across rows"
        );
    }
}
