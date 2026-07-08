//! Terminal display-width helpers — CJK / emoji / combining aware.
//!
//! 终端宽度辅助函数 — 兼容 CJK 全角、emoji、ZWJ、组合字符。
//!
//! Wraps `unicode-width` so callers don't need to track its trait surface and
//! so `--no-color` / pipe and wide-char evolution stay in one place.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display width of the input in terminal columns, treating zero-width
/// joiners and combining marks as zero-width (matching common terminal
/// behavior).
#[must_use]
pub fn display_width(s: &str) -> usize {
    // `UnicodeWidthStr::width` already accounts for combined grapheme widths
    // better than per-char summation; we use it directly.
    s.width()
}

/// Per-character terminal column width, summed. Useful when the caller has
/// already split a string into characters and wants to avoid re-parsing.
#[must_use]
pub fn char_widths(s: &str) -> Vec<usize> {
    s.chars()
        .map(|c| c.width().unwrap_or(0))
        .collect::<Vec<_>>()
}

/// Pad `s` with trailing spaces so its terminal display width equals `target`,
/// iff `s` is narrower. Wider inputs are returned unchanged.
#[must_use]
pub fn pad_to_width(s: &str, target: usize) -> String {
    let w = display_width(s);
    if w >= target {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(target - w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_width_handles_cjk_wide_chars() {
        // CJK 全角字符每个占 2 列
        assert_eq!(display_width("你好"), 4);
        assert_eq!(display_width("a你b"), 4);
    }

    #[test]
    fn display_width_handles_emoji_zwj_sequence() {
        // emoji 列宽依赖 Unicode 版本：unicode-width 0.2 多数 ZWJ emoji 算 1 或 2
        // 列宽。这里只断言 ZWJ/FE0F 自身 0 宽，以及 emoji 非 0。
        assert!(display_width("🦀") >= 1);
        assert_eq!(display_width("\u{200d}"), 0);
        assert_eq!(display_width("\u{fe0f}"), 0);
    }

    #[test]
    fn display_width_handles_half_full_width_mix() {
        // 半角 + 全角 + 半角
        assert_eq!(display_width("A你B好C"), 7);
        // 纯半角
        assert_eq!(display_width("ABC"), 3);
        // 纯全角数字（U+FF11..）
        assert_eq!(display_width("１２"), 4);
    }

    #[test]
    fn char_widths_matches_per_char() {
        let v = char_widths("你a🦀");
        assert_eq!(v, vec![2, 1, 2]);
    }

    #[test]
    fn pad_to_width_no_pad_when_wider_than_target() {
        assert_eq!(pad_to_width("你好", 2), "你好");
        assert_eq!(pad_to_width("你", 4), "你  ");
        assert_eq!(pad_to_width("abc", 5), "abc  ");
    }
}
