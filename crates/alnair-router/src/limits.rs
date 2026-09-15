//! In-process limits: upstream concurrency caps and per-key rate limiting.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::config::{LimitsConfig, RateLimitConfig};
use crate::error::{Error, Result};

/// A held pair of concurrency slots, released when dropped.
#[derive(Debug)]
pub struct LimitPermit {
    _global: Option<OwnedSemaphorePermit>,
    _connection: Option<OwnedSemaphorePermit>,
}

/// Caps concurrent upstream calls globally and per connection.
#[derive(Clone)]
pub struct UpstreamLimiter {
    global: Option<Arc<Semaphore>>,
    per_connection_limit: usize,
    per_connection: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    timeout: Option<Duration>,
}

impl UpstreamLimiter {
    pub fn new(config: &LimitsConfig) -> Self {
        Self {
            global: (config.max_concurrent > 0)
                .then(|| Arc::new(Semaphore::new(config.max_concurrent))),
            per_connection_limit: config.max_concurrent_per_connection,
            per_connection: Arc::new(Mutex::new(HashMap::new())),
            timeout: (config.acquire_timeout_ms > 0)
                .then(|| Duration::from_millis(config.acquire_timeout_ms)),
        }
    }

    /// True when either cap is configured; with none, `acquire` is free.
    pub fn is_enabled(&self) -> bool {
        self.global.is_some() || self.per_connection_limit > 0
    }

    /// Acquires a global slot and a per-connection slot.
    ///
    /// Waits up to `limits.acquire_timeout_ms`; a request that cannot get a
    /// slot in time fails with `429 Too Many Requests`.
    pub async fn acquire(&self, connection_key: &str) -> Result<LimitPermit> {
        if !self.is_enabled() {
            return Ok(LimitPermit {
                _global: None,
                _connection: None,
            });
        }

        match self.timeout {
            Some(timeout) => tokio::time::timeout(timeout, self.acquire_inner(connection_key))
                .await
                .map_err(|_| Error::RateLimited {
                    message: "upstream concurrency limit reached".to_string(),
                    retry_after_secs: 1,
                })?,
            None => self.acquire_inner(connection_key).await,
        }
    }

    async fn acquire_inner(&self, connection_key: &str) -> Result<LimitPermit> {
        let global = match &self.global {
            Some(semaphore) => Some(
                semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| limiter_closed())?,
            ),
            None => None,
        };

        let connection = if self.per_connection_limit > 0 {
            let semaphore = self.semaphore_for(connection_key);
            Some(
                semaphore
                    .acquire_owned()
                    .await
                    .map_err(|_| limiter_closed())?,
            )
        } else {
            None
        };

        Ok(LimitPermit {
            _global: global,
            _connection: connection,
        })
    }

    fn semaphore_for(&self, connection_key: &str) -> Arc<Semaphore> {
        let mut map = self.per_connection.lock().expect("limiter map poisoned");
        map.entry(connection_key.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.per_connection_limit)))
            .clone()
    }
}

fn limiter_closed() -> Error {
    Error::Internal("upstream limiter was closed".to_string())
}

/// How a key's monthly budget behaves once exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BudgetMode {
    #[default]
    Off,
    Warn,
    Block,
}

impl BudgetMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetMode::Off => "off",
            BudgetMode::Warn => "warn",
            BudgetMode::Block => "block",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "off" => Ok(BudgetMode::Off),
            "warn" => Ok(BudgetMode::Warn),
            "block" => Ok(BudgetMode::Block),
            other => Err(Error::BadRequest(format!(
                "budget_mode must be one of off, warn, block (got '{other}')"
            ))),
        }
    }
}

/// Token-bucket limiter keyed by API key id.
///
/// A key's effective rate is its own `rate_limit_per_minute` when set,
/// otherwise the global default. Zero means unlimited.
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
    default_rpm: u32,
    burst: u32,
}

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(config: &RateLimitConfig) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            default_rpm: config.requests_per_minute,
            burst: config.burst,
        }
    }

    /// Consumes one token for `key_id`, or fails with `429`.
    pub fn check(&self, key_id: &str, override_rpm: Option<u32>) -> Result<()> {
        self.check_at(key_id, override_rpm, Instant::now())
    }

    /// Testable variant with an injectable clock.
    fn check_at(&self, key_id: &str, override_rpm: Option<u32>, now: Instant) -> Result<()> {
        let rpm = override_rpm
            .filter(|value| *value > 0)
            .unwrap_or(self.default_rpm);
        if rpm == 0 {
            return Ok(());
        }

        let capacity = if self.burst > 0 {
            self.burst as f64
        } else {
            rpm as f64
        };
        let rate_per_sec = rpm as f64 / 60.0;

        let mut buckets = self.buckets.lock().expect("rate limiter poisoned");
        let bucket = buckets.entry(key_id.to_string()).or_insert(TokenBucket {
            tokens: capacity,
            last_refill: now,
        });

        let elapsed = now
            .saturating_duration_since(bucket.last_refill)
            .as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * rate_per_sec).min(capacity);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            return Ok(());
        }

        let deficit = 1.0 - bucket.tokens;
        let retry_after_secs = (deficit / rate_per_sec).ceil().max(1.0) as u64;
        Err(Error::RateLimited {
            message: format!("rate limit exceeded ({rpm} requests/minute)"),
            retry_after_secs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(max_concurrent: usize, per_connection: usize, timeout_ms: u64) -> LimitsConfig {
        LimitsConfig {
            max_concurrent,
            max_concurrent_per_connection: per_connection,
            acquire_timeout_ms: timeout_ms,
        }
    }

    #[tokio::test]
    async fn disabled_limiter_never_blocks() {
        let limiter = UpstreamLimiter::new(&limits(0, 0, 0));
        assert!(!limiter.is_enabled());

        let first = limiter.acquire("a").await.expect("permit");
        let second = limiter.acquire("a").await.expect("permit");
        drop((first, second));
    }

    #[tokio::test]
    async fn global_cap_times_out_and_recovers() {
        let limiter = UpstreamLimiter::new(&limits(1, 0, 50));

        let held = limiter.acquire("a").await.expect("first permit");
        let blocked = limiter.acquire("b").await;
        assert!(matches!(blocked, Err(Error::RateLimited { .. })));

        drop(held);
        limiter.acquire("b").await.expect("slot freed");
    }

    #[tokio::test]
    async fn per_connection_cap_is_isolated() {
        let limiter = UpstreamLimiter::new(&limits(0, 1, 50));

        let first = limiter.acquire("a").await.expect("connection a");
        let other = limiter.acquire("b").await.expect("connection b");
        let blocked = limiter.acquire("a").await;
        assert!(matches!(blocked, Err(Error::RateLimited { .. })));

        drop((first, other));
    }

    fn rate_limiter(rpm: u32, burst: u32) -> RateLimiter {
        RateLimiter::new(&RateLimitConfig {
            requests_per_minute: rpm,
            burst,
        })
    }

    #[test]
    fn unlimited_when_rate_is_zero() {
        let limiter = rate_limiter(0, 0);
        assert!(limiter.check("key", None).is_ok());
        assert!(limiter.check("key", Some(0)).is_ok());
    }

    #[test]
    fn burst_is_consumed_then_refilled() {
        let limiter = rate_limiter(60, 1);
        let start = Instant::now();

        assert!(limiter.check_at("key", None, start).is_ok());

        let blocked = limiter.check_at("key", None, start);
        match blocked {
            Err(Error::RateLimited {
                retry_after_secs, ..
            }) => assert_eq!(retry_after_secs, 1),
            other => panic!("expected rate limit, got {other:?}"),
        }

        // One second at 60 rpm refills exactly one token.
        assert!(
            limiter
                .check_at("key", None, start + Duration::from_secs(1))
                .is_ok()
        );
    }

    #[test]
    fn per_key_override_beats_the_default() {
        let limiter = rate_limiter(0, 1);
        let start = Instant::now();

        // Default is unlimited, but the override applies.
        assert!(limiter.check_at("key", Some(60), start).is_ok());
        assert!(limiter.check_at("key", Some(60), start).is_err());
        // Another key without the override is unaffected.
        assert!(limiter.check_at("other", None, start).is_ok());
    }

    #[test]
    fn budget_mode_parses() {
        assert_eq!(BudgetMode::parse("off").expect("off"), BudgetMode::Off);
        assert_eq!(BudgetMode::parse(" WARN ").expect("warn"), BudgetMode::Warn);
        assert_eq!(
            BudgetMode::parse("block").expect("block"),
            BudgetMode::Block
        );
        assert!(BudgetMode::parse("nope").is_err());
    }
}
