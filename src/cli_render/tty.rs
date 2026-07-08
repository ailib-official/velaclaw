//! TTY detection for the CLI render layer.
//!
//! 渲染层 TTY 检测 — `stdout` 是终端时启用 ANSI，否则剥离以适配管道与 CI。

use std::io::IsTerminal;

/// True iff we should emit ANSI escapes. Decided once per process invocation
/// (kept cheap; `is_terminal()` is a syscall fast-path on most platforms).
///
/// Honors the `NO_COLOR` env convention (`https://no-color.org`): when set,
/// ANSI is suppressed regardless of TTY status.
#[must_use]
pub fn ansi_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env-mutating tests to avoid cross-test interference.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn ansi_enabled_respects_no_color_env() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Save and restore — keep tests hermetic.
        let saved = std::env::var_os("NO_COLOR");
        std::env::set_var("NO_COLOR", "1");
        assert!(!ansi_enabled());
        // Restore
        if let Some(v) = saved {
            std::env::set_var("NO_COLOR", v);
        } else {
            std::env::remove_var("NO_COLOR");
        }
    }
}
