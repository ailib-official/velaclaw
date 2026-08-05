//! Process-local session model overrides (ORCH-HOST-001).
//!
//! Candidates must still pass CAP reachable checks in [`super::host_decide`].

use super::host_decide::SessionModelOverride;
use std::collections::HashMap;
use std::sync::Mutex;

static OVERRIDES: Mutex<Option<HashMap<String, SessionModelOverride>>> = Mutex::new(None);

fn map() -> std::sync::MutexGuard<'static, Option<HashMap<String, SessionModelOverride>>> {
    OVERRIDES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Set or clear override for `session_key` (`""` = default/global agent session).
pub fn set_override(session_key: &str, next: Option<SessionModelOverride>) {
    let mut guard = map();
    let store = guard.get_or_insert_with(HashMap::new);
    match next {
        Some(v) => {
            store.insert(session_key.to_string(), v);
        }
        None => {
            store.remove(session_key);
        }
    }
}

#[must_use]
pub fn get_override(session_key: &str) -> Option<SessionModelOverride> {
    let guard = map();
    guard.as_ref()?.get(session_key).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_clear() {
        set_override(
            "t1",
            Some(SessionModelOverride {
                provider_id: "groq".into(),
                model: "llama".into(),
            }),
        );
        let got = get_override("t1").expect("ov");
        assert_eq!(got.provider_id, "groq");
        set_override("t1", None);
        assert!(get_override("t1").is_none());
    }
}
