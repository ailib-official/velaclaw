//! Ephemeral one-shot secret slots for interactive human input (HITL).
//! 交互式密钥槽：仅存本机内存，单次取出后销毁，从不进入模型上下文。

use parking_lot::Mutex;
use std::collections::HashMap;
use uuid::Uuid;

/// In-memory one-shot secrets referenced by opaque slot ids.
#[derive(Default)]
pub struct SecretSlotStore {
    slots: Mutex<HashMap<String, String>>,
}

impl SecretSlotStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a secret and return an opaque slot id (never log the value).
    pub fn put(&self, secret: String) -> String {
        let id = Uuid::new_v4().to_string();
        self.slots.lock().insert(id.clone(), secret);
        id
    }

    /// Take and destroy a secret. Returns `None` if missing or already consumed.
    pub fn take(&self, slot_id: &str) -> Option<String> {
        self.slots.lock().remove(slot_id)
    }

    pub fn len(&self) -> usize {
        self.slots.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_take_is_one_shot() {
        let store = SecretSlotStore::new();
        let id = store.put("s3cret".into());
        assert_eq!(store.take(&id).as_deref(), Some("s3cret"));
        assert!(store.take(&id).is_none());
    }
}
