//! Double-Esc detector and stdin watcher for interactive CLI cancel (VL-UX-CANCEL-001).
//! 交互 CLI：连按两次 Esc 取消当前 turn。

use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Default window between two Esc presses.
pub const DOUBLE_ESC_WINDOW: Duration = Duration::from_millis(500);

/// Pure detector: two Esc events within `window` trigger cancel.
#[derive(Debug, Default)]
pub struct DoubleEscDetector {
    last_esc: Option<Instant>,
    window: Duration,
}

impl DoubleEscDetector {
    #[must_use]
    pub fn new(window: Duration) -> Self {
        Self {
            last_esc: None,
            window,
        }
    }

    /// Record an Esc at `now`. Returns true if this completes a double-Esc.
    pub fn on_esc(&mut self, now: Instant) -> bool {
        if let Some(prev) = self.last_esc {
            if now.saturating_duration_since(prev) <= self.window {
                self.last_esc = None;
                return true;
            }
        }
        self.last_esc = Some(now);
        false
    }

    /// Non-Esc input clears the pending first Esc.
    pub fn reset(&mut self) {
        self.last_esc = None;
    }
}

/// Watch stdin for double Esc until `token` is cancelled (turn ended or already stopped).
///
/// No-op when stdin is not a TTY. Unix only (raw poll); other platforms skip.
pub fn spawn_double_esc_watcher(token: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        #[cfg(unix)]
        watch_unix(token);
        #[cfg(not(unix))]
        {
            let _ = token;
        }
    })
}

#[cfg(unix)]
fn stdin_is_tty() -> bool {
    // SAFETY: isatty on STDIN_FILENO is always valid.
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

#[cfg(unix)]
struct RawMode {
    original: libc::termios,
}

#[cfg(unix)]
impl RawMode {
    fn enable() -> Option<Self> {
        unsafe {
            let mut ios = std::mem::zeroed::<libc::termios>();
            // SAFETY: stdin fd + termios buffer owned here.
            if libc::tcgetattr(libc::STDIN_FILENO, &raw mut ios) != 0 {
                return None;
            }
            let original = ios;
            ios.c_lflag &= !(libc::ICANON | libc::ECHO);
            ios.c_cc[libc::VMIN] = 1;
            ios.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const ios) != 0 {
                return None;
            }
            Some(Self { original })
        }
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: restore termios captured at enable().
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const self.original);
        }
    }
}

#[cfg(unix)]
fn poll_stdin(timeout_ms: i32) -> bool {
    let mut fds = [libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    }];
    // SAFETY: pollfd array is local and fd is STDIN.
    unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout_ms) > 0 }
}

#[cfg(unix)]
fn read_byte() -> Option<u8> {
    let mut buf = [0u8; 1];
    // SAFETY: 1-byte stack buffer.
    let n = unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr().cast(), 1) };
    if n == 1 {
        Some(buf[0])
    } else {
        None
    }
}

#[cfg(unix)]
fn watch_unix(token: CancellationToken) {
    if !stdin_is_tty() {
        return;
    }
    let Some(_raw) = RawMode::enable() else {
        return;
    };
    let mut det = DoubleEscDetector::new(DOUBLE_ESC_WINDOW);
    while !token.is_cancelled() {
        if !poll_stdin(100) {
            continue;
        }
        let Some(b) = read_byte() else {
            continue;
        };
        if b != 0x1b {
            det.reset();
            continue;
        }
        // If more bytes follow immediately, this is an escape sequence (arrows), not Esc.
        if poll_stdin(40) {
            while poll_stdin(0) {
                let _ = read_byte();
            }
            det.reset();
            continue;
        }
        if det.on_esc(Instant::now()) {
            token.cancel();
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_esc_within_window_triggers() {
        let mut d = DoubleEscDetector::new(Duration::from_millis(500));
        let t0 = Instant::now();
        assert!(!d.on_esc(t0));
        assert!(d.on_esc(t0 + Duration::from_millis(200)));
    }

    #[test]
    fn double_esc_outside_window_does_not_trigger() {
        let mut d = DoubleEscDetector::new(Duration::from_millis(500));
        let t0 = Instant::now();
        assert!(!d.on_esc(t0));
        assert!(!d.on_esc(t0 + Duration::from_millis(800)));
    }

    #[test]
    fn reset_clears_pending_esc() {
        let mut d = DoubleEscDetector::new(Duration::from_millis(500));
        let t0 = Instant::now();
        assert!(!d.on_esc(t0));
        d.reset();
        assert!(!d.on_esc(t0 + Duration::from_millis(100)));
    }
}
