//! Per-key concurrency accounting shared by request lifetimes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};

/// Counts active requests, including requests started before a limit changes.
#[derive(Clone, Default)]
pub(crate) struct KeyConcurrency {
    active: Arc<Mutex<HashMap<String, usize>>>,
}

/// A request owns its slot until the response body is consumed or dropped.
pub(crate) struct KeyPermit {
    active: Arc<Mutex<HashMap<String, usize>>>,
    key_id: String,
}

impl KeyConcurrency {
    /// A zero limit is unlimited, but still counts towards a future limit.
    pub(crate) fn acquire(&self, key_id: &str, limit: u64) -> Result<KeyPermit> {
        let mut active = self.active.lock().expect("key concurrency lock poisoned");
        let count = active.entry(key_id.to_string()).or_default();
        if limit != 0 && (*count as u64) >= limit {
            return Err(Error::RateLimited {
                message: "API key concurrency limit reached".to_string(),
                retry_after_secs: 1,
            });
        }
        *count += 1;
        Ok(KeyPermit {
            active: Arc::clone(&self.active),
            key_id: key_id.to_string(),
        })
    }
}

impl Drop for KeyPermit {
    fn drop(&mut self) {
        let mut active = self.active.lock().expect("key concurrency lock poisoned");
        if let Some(count) = active.get_mut(&self.key_id) {
            *count -= 1;
            if *count == 0 {
                active.remove(&self.key_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_each_key_independently_and_releases_on_drop() {
        let limiter = KeyConcurrency::default();
        let first = limiter.acquire("first", 1).unwrap();
        assert!(limiter.acquire("first", 1).is_err());
        let second = limiter.acquire("second", 1).unwrap();
        drop(first);
        assert!(limiter.acquire("first", 1).is_ok());
        assert!(limiter.acquire("second", 1).is_err());
        drop(second);
        assert!(limiter.acquire("second", 1).is_ok());
    }

    #[test]
    fn lowering_a_limit_counts_existing_unlimited_requests() {
        let limiter = KeyConcurrency::default();
        let first = limiter.acquire("key", 0).unwrap();
        let second = limiter.acquire("key", 0).unwrap();
        assert!(limiter.acquire("key", 1).is_err());
        drop(first);
        assert!(limiter.acquire("key", 1).is_err());
        drop(second);
        assert!(limiter.acquire("key", 1).is_ok());
    }
}
