//! Round-robin rotation for `/v1/search` fallback tiers.
//!
//! Search connections are grouped by `search_tier`. Lower tiers are tried
//! first. Inside a tier, each request starts at the next connection in turn, so
//! load spreads across peers instead of always hitting the first one. The
//! counters live in memory and reset when the router restarts, which is enough
//! to spread load without a database write per request.

use std::collections::HashMap;
use std::sync::Mutex;

/// Per-tier round-robin counters shared by every search request.
#[derive(Debug, Default)]
pub struct SearchRotation {
    cursors: Mutex<HashMap<i64, usize>>,
}

impl SearchRotation {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the tier's connections in the order to try them for this
    /// request, starting from the next cursor position.
    ///
    /// `members` must already be sorted, so the rotation is stable across
    /// requests. An empty tier returns an empty order.
    pub fn order<T: Clone>(&self, tier: i64, members: &[T]) -> Vec<T> {
        if members.is_empty() {
            return Vec::new();
        }

        let start = {
            let mut cursors = self.cursors.lock().expect("search rotation lock poisoned");
            let cursor = cursors.entry(tier).or_insert(0);
            let start = *cursor % members.len();
            *cursor = cursor.wrapping_add(1);
            start
        };

        members
            .iter()
            .cycle()
            .skip(start)
            .take(members.len())
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_member_is_always_first() {
        let rotation = SearchRotation::new();
        assert_eq!(rotation.order(1, &["only"]), vec!["only"]);
        assert_eq!(rotation.order(1, &["only"]), vec!["only"]);
    }

    #[test]
    fn requests_rotate_through_the_tier() {
        let rotation = SearchRotation::new();
        let members = ["a", "b", "c"];

        assert_eq!(rotation.order(1, &members), vec!["a", "b", "c"]);
        assert_eq!(rotation.order(1, &members), vec!["b", "c", "a"]);
        assert_eq!(rotation.order(1, &members), vec!["c", "a", "b"]);
        assert_eq!(rotation.order(1, &members), vec!["a", "b", "c"]);
    }

    #[test]
    fn each_tier_keeps_its_own_cursor() {
        let rotation = SearchRotation::new();
        assert_eq!(rotation.order(1, &["a", "b"]), vec!["a", "b"]);
        assert_eq!(rotation.order(2, &["x", "y"]), vec!["x", "y"]);
        assert_eq!(rotation.order(1, &["a", "b"]), vec!["b", "a"]);
    }

    #[test]
    fn an_empty_tier_yields_nothing() {
        let rotation = SearchRotation::new();
        let empty: [&str; 0] = [];
        assert!(rotation.order(1, &empty).is_empty());
    }
}
