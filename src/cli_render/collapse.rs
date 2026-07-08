//! Long-output collapse for the CLI render layer.
//!
//! 长输出折叠工具 — 折叠 fenced code 与 tool 块到前 N 行 + `/expand <id>` 可回放 payload。

/// Result of a collapse decision.
///
/// `Folded`：
/// - `visible` — 输出前 `n` 行 + 折叠 footer
/// - `payload` — 完整原文，调用方维护 `HashMap<id, String>` 以便 `/expand <id>` 回放
/// - `id` — 折叠块 ID（仅当确实折叠时存在）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folded<'a> {
    pub visible: String,
    pub payload: Option<&'a str>,
    pub id: Option<u64>,
    pub total_lines: usize,
    pub collapsed_after: usize,
}

/// Default footer hint pointing the user to REPL `/expand`.
const EXPAND_HINT_PREFIX: &str = "用 /expand";

/// Decide whether `text` exceeds `fold_lines` and, if so, return a stub:
///
/// ```text
/// [前 fold_lines 行 / 共 N 行]
/// <visible lines>
/// ─────
/// 用 /expand <id> 展开全部
/// ```
///
/// When not collapsing, `payload` and `id` are `None` and `visible` carries the
/// original `text` (no footer overhead).
///
/// `id` is provided by the caller so the REPL dispatcher controls ID numbering.
/// Pass `Some(id)` to opt into fold behavior; `None` keeps the output fully
/// expanded (for non-interactive / `--no-fold` paths).
pub fn fold(text: &str, fold_lines: usize, id: Option<u64>) -> Folded<'_> {
    let lines: Vec<&str> = text.split('\n').collect();
    let total = lines.len();
    let Some(id_v) = id else {
        return Folded {
            visible: text.to_string(),
            payload: None,
            id: None,
            total_lines: total,
            collapsed_after: total,
        };
    };
    if total <= fold_lines {
        return Folded {
            visible: text.to_string(),
            payload: None,
            id: None,
            total_lines: total,
            collapsed_after: total,
        };
    }
    let head: Vec<&str> = lines.into_iter().take(fold_lines).collect();
    let head_block = head.join("\n");
    let visible = format!(
        "[前 {fold_lines} 行 / 共 {total} 行]\n{head_block}\n─────\n{EXPAND_HINT_PREFIX} {id_v} 展开全部"
    );
    Folded {
        visible,
        payload: Some(text),
        id: Some(id_v),
        total_lines: total,
        collapsed_after: fold_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_collapses_at_fold_lines_threshold() {
        let text = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12";
        let r = fold(text, 10, Some(1));
        assert_eq!(r.total_lines, 12);
        assert_eq!(r.collapsed_after, 10);
        assert_eq!(r.id, Some(1));
        assert!(r.visible.contains("前 10 行 / 共 12 行"));
        assert!(r.visible.contains("l10"));
        assert!(!r.visible.contains("\nl11\n"));
        assert!(r.visible.contains("/expand 1"));
        assert_eq!(r.payload, Some(text));
    }

    #[test]
    fn fold_skips_when_below_threshold() {
        let text = "a\nb\nc";
        let r = fold(text, 10, Some(1));
        assert_eq!(r.visible, text);
        assert_eq!(r.id, None);
        assert_eq!(r.payload, None);
    }

    #[test]
    fn fold_at_exact_threshold_no_collapse() {
        let text = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10";
        let r = fold(text, 10, Some(1));
        // Equal to threshold means not exceeding → no fold.
        assert_eq!(r.visible, text);
        assert_eq!(r.id, None);
    }

    #[test]
    fn fold_skipped_when_id_none() {
        // Non-interactive / --no-fold path: caller passes None.
        let text = "a\nb\n".repeat(20);
        let r = fold(&text, 10, None);
        assert_eq!(r.visible, text);
        assert_eq!(r.id, None);
        assert_eq!(r.payload, None);
    }

    #[test]
    fn fold_payload_preserves_full_text_for_expand_replay() {
        let text = "line\n".repeat(20);
        let trimmed = text.trim_end_matches('\n');
        let r = fold(trimmed, 5, Some(7));
        assert_eq!(r.payload, Some(trimmed));
        assert!(r.visible.contains("/expand 7"));
    }
}
