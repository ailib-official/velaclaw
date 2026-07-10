//! REPL speaker prefixes — user `>` vs agent `>>` (applied after Markdown render).

use crate::cli_render::markdown::RenderStyle;

/// Highlighted `> ` prompt for user input (interactive REPL).
#[must_use]
pub fn format_user_prompt(style: RenderStyle) -> String {
    if style.ansi {
        "\x1b[1;36m>\x1b[0m ".to_string()
    } else {
        "> ".to_string()
    }
}

/// Prefix only the first non-empty line with highlighted `>> ` for agent turns.
#[must_use]
pub fn prefix_agent_lines(text: &str, style: RenderStyle) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut prefixed_first = false;
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else if !prefixed_first {
                prefixed_first = true;
                if style.ansi {
                    format!("\x1b[1;92m>>\x1b[0m {line}")
                } else {
                    format!(">> {line}")
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Indent every line (tool blocks, nested output). Blank lines stay blank.
#[must_use]
pub fn indent_lines(text: &str, spaces: usize) -> String {
    if text.is_empty() {
        return String::new();
    }
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{pad}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ansi_on() -> RenderStyle {
        RenderStyle {
            ansi: true,
            markdown: true,
        }
    }

    #[test]
    fn user_prompt_has_cyan_marker_when_ansi() {
        let p = format_user_prompt(ansi_on());
        assert!(p.contains("\x1b[1;36m>"));
    }

    #[test]
    fn agent_turn_prefixes_only_first_line() {
        let out = prefix_agent_lines("line one\nline two", ansi_on());
        assert!(out.starts_with("\x1b[1;92m>>\x1b[0m line one"));
        assert!(out.contains("\nline two"));
        assert!(!out.contains("\x1b[1;92m>>\x1b[0m line two"));
    }

    #[test]
    fn indent_lines_adds_padding() {
        let out = indent_lines("a\n\nb", 2);
        assert_eq!(out, "  a\n\n  b");
    }
}
