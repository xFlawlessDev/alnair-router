//! Atomic per-key token reservations for a rolling one-minute window.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
struct Bucket {
    completed: VecDeque<(Instant, u64)>,
    reserved: u64,
}

#[derive(Clone, Default)]
pub(crate) struct TokenRateLimiter {
    buckets: Arc<Mutex<HashMap<String, Bucket>>>,
}

/// A reservation charges its estimate when dropped, unless usage replaces it.
#[derive(Debug)]
pub(crate) struct TokenReservation {
    buckets: Arc<Mutex<HashMap<String, Bucket>>>,
    key_id: String,
    estimate: u64,
    actual: Option<u64>,
}

pub(crate) type SharedReservation = Arc<Mutex<TokenReservation>>;

pub(crate) fn reconcile(
    reservation: &Option<SharedReservation>,
    usage: Option<crate::upstream::chat_backend::TokenUsage>,
) {
    if let (Some(reservation), Some(usage)) = (reservation, usage) {
        reservation
            .lock()
            .expect("token reservation lock poisoned")
            .reconcile(usage.prompt_tokens.saturating_add(usage.completion_tokens));
    }
}

impl TokenRateLimiter {
    pub(crate) fn reserve(
        &self,
        key_id: &str,
        limit: u64,
        estimate: u64,
    ) -> Result<TokenReservation> {
        self.reserve_at(key_id, limit, estimate, Instant::now())
    }

    fn reserve_at(
        &self,
        key_id: &str,
        limit: u64,
        estimate: u64,
        now: Instant,
    ) -> Result<TokenReservation> {
        let mut buckets = self.buckets.lock().expect("token rate lock poisoned");
        let bucket = buckets.entry(key_id.to_string()).or_default();
        while bucket
            .completed
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) >= WINDOW)
        {
            bucket.completed.pop_front();
        }
        let used = bucket
            .completed
            .iter()
            .fold(bucket.reserved, |sum, (_, tokens)| {
                sum.saturating_add(*tokens)
            });
        if limit > 0 && used.saturating_add(estimate) > limit {
            let retry_after_secs = bucket
                .completed
                .front()
                .map(|(at, _)| {
                    WINDOW
                        .saturating_sub(now.saturating_duration_since(*at))
                        .as_secs()
                        .saturating_add(1)
                })
                .unwrap_or(1);
            return Err(Error::RateLimited {
                message: "API key tokens-per-minute limit reached".to_string(),
                retry_after_secs,
            });
        }
        bucket.reserved = bucket.reserved.saturating_add(estimate);
        Ok(TokenReservation {
            buckets: Arc::clone(&self.buckets),
            key_id: key_id.to_string(),
            estimate,
            actual: None,
        })
    }
}

impl TokenReservation {
    /// Replaces the estimate with provider-reported prompt plus output tokens.
    pub(crate) fn reconcile(&mut self, actual: u64) {
        self.actual = Some(actual);
    }

    fn finish_at(&mut self, now: Instant) {
        let mut buckets = self.buckets.lock().expect("token rate lock poisoned");
        if let Some(bucket) = buckets.get_mut(&self.key_id) {
            bucket.reserved = bucket.reserved.saturating_sub(self.estimate);
            bucket
                .completed
                .push_back((now, self.actual.unwrap_or(self.estimate)));
        }
    }
}

impl Drop for TokenReservation {
    fn drop(&mut self) {
        self.finish_at(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_reservations_count_and_keys_are_isolated() {
        let limiter = TokenRateLimiter::default();
        let first = limiter.reserve("first", 10, 6).unwrap();
        assert!(limiter.reserve("first", 10, 5).is_err());
        assert!(limiter.reserve("second", 10, 6).is_ok());
        drop(first);
        assert!(limiter.reserve("first", 10, 5).is_err());
    }

    #[test]
    fn reconciliation_replaces_estimate_with_actual_usage() {
        let limiter = TokenRateLimiter::default();
        let mut reservation = limiter.reserve("key", 10, 8).unwrap();
        reservation.reconcile(3);
        drop(reservation);
        assert!(limiter.reserve("key", 10, 7).is_ok());
        assert!(limiter.reserve("key", 10, 8).is_err());
    }

    #[test]
    fn completed_usage_expires_after_one_minute() {
        let limiter = TokenRateLimiter::default();
        let now = Instant::now();
        let reservation = limiter.reserve_at("key", 10, 10, now).unwrap();
        drop(reservation);
        assert!(
            limiter
                .reserve_at("key", 10, 1, now + Duration::from_secs(61))
                .is_ok()
        );
    }

    #[test]
    fn zero_limit_remains_unlimited_and_is_counted() {
        let limiter = TokenRateLimiter::default();
        let reservation = limiter.reserve("key", 0, 10).unwrap();
        assert!(limiter.reserve("key", 5, 1).is_err());
        drop(reservation);
        assert!(limiter.reserve("key", 5, 1).is_err());
    }

    #[test]
    fn simultaneous_reservations_cannot_exceed_the_limit() {
        let limiter = TokenRateLimiter::default();
        let barrier = Arc::new(std::sync::Barrier::new(9));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let limiter = limiter.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    limiter.reserve("key", 10, 6).ok()
                })
            })
            .collect();
        barrier.wait();
        let reservations: Vec<_> = threads
            .into_iter()
            .filter_map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(reservations.len(), 1);
    }
}
