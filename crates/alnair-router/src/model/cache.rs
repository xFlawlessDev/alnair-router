//! Cached routing catalog with TTL and explicit invalidation.
//!
//! `Catalog::load` reads every connection, alias, combo and entry from SQLite
//! and decrypts connection credentials; doing that on every request is the
//! first thing that hurts under load. This cache serves a shared snapshot until
//! `invalidate` is called (admin writes do that) or the TTL lapses, whichever
//! comes first.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use tokio::sync::RwLock;

use super::resolver::{Catalog, Resolver};
use crate::config::RouterConfig;
use crate::crypto::CredentialCipher;
use crate::error::Result;

/// A catalog snapshot and the resolver built from it.
#[derive(Clone)]
pub struct CatalogSnapshot {
    pub catalog: Arc<Catalog>,
    pub resolver: Arc<Resolver>,
}

struct Cached {
    catalog: Arc<Catalog>,
    resolver: Arc<Resolver>,
    loaded_at: Instant,
}

/// Routing knobs read from the live config each time a snapshot is built.
#[derive(Clone)]
struct CacheOptions {
    default_connection: Option<String>,
    max_attempts: usize,
    ttl: Duration,
}

impl CacheOptions {
    fn from_config(config: &RouterConfig) -> Self {
        Self {
            default_connection: config.router.default_connection.clone(),
            max_attempts: config.router.max_attempts,
            ttl: Duration::from_millis(config.router.catalog_ttl_ms),
        }
    }
}

/// Caches the routing catalog.
pub struct CatalogCache {
    pool: SqlitePool,
    cipher: Arc<CredentialCipher>,
    options: std::sync::RwLock<CacheOptions>,
    inner: RwLock<Option<Cached>>,
}

impl CatalogCache {
    pub fn new(config: &RouterConfig, pool: SqlitePool, cipher: Arc<CredentialCipher>) -> Self {
        Self {
            pool,
            cipher,
            options: std::sync::RwLock::new(CacheOptions::from_config(config)),
            inner: RwLock::new(None),
        }
    }

    /// Replaces the routing knobs and drops the snapshot so the next request
    /// rebuilds its resolver from the new values.
    pub async fn apply(&self, config: &RouterConfig) {
        {
            let mut options = self
                .options
                .write()
                .expect("catalog cache options poisoned");
            *options = CacheOptions::from_config(config);
        }
        self.invalidate().await;
    }

    /// Returns the cached snapshot, reloading it when stale.
    pub async fn snapshot(&self) -> Result<CatalogSnapshot> {
        if let Some(cached) = self.fresh_guard().await {
            return Ok(cached);
        }

        // Take the write lock and re-check: another task may have just loaded.
        let mut guard = self.inner.write().await;
        if let Some(cached) = guard.as_ref().filter(|cached| self.is_fresh(cached)) {
            return Ok(snapshot_of(cached));
        }

        let options = self.options();
        let catalog = Arc::new(Catalog::load(&self.pool, &self.cipher).await?);
        let resolver =
            Arc::new(catalog.resolver(options.default_connection.clone(), options.max_attempts));
        *guard = Some(Cached {
            catalog: catalog.clone(),
            resolver: resolver.clone(),
            loaded_at: Instant::now(),
        });

        Ok(CatalogSnapshot { catalog, resolver })
    }

    /// Drops the cached snapshot so the next request reloads it.
    pub async fn invalidate(&self) {
        *self.inner.write().await = None;
    }

    async fn fresh_guard(&self) -> Option<CatalogSnapshot> {
        let guard = self.inner.read().await;
        guard
            .as_ref()
            .filter(|cached| self.is_fresh(cached))
            .map(snapshot_of)
    }

    /// A zero TTL means "cache until explicitly invalidated".
    fn is_fresh(&self, cached: &Cached) -> bool {
        let ttl = self.options().ttl;
        ttl.is_zero() || cached.loaded_at.elapsed() < ttl
    }

    fn options(&self) -> CacheOptions {
        self.options
            .read()
            .expect("catalog cache options poisoned")
            .clone()
    }
}

fn snapshot_of(cached: &Cached) -> CatalogSnapshot {
    CatalogSnapshot {
        catalog: cached.catalog.clone(),
        resolver: cached.resolver.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::db::repos::connections::{ConnectionRepository, CreateConnection};

    fn config(ttl_ms: u64) -> RouterConfig {
        RouterConfig {
            secrets: crate::config::SecretsConfig {
                key: Some("ab".repeat(32)),
            },
            router: crate::config::RoutingConfig {
                catalog_ttl_ms: ttl_ms,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    async fn cache(ttl_ms: u64) -> (CatalogCache, Db, Arc<CredentialCipher>) {
        let db = Db::connect_in_memory().await.expect("db");
        let config = config(ttl_ms);
        let cipher = Arc::new(CredentialCipher::from_config(&config.secrets).expect("cipher"));
        let cache = CatalogCache::new(&config, db.pool.clone(), cipher.clone());
        (cache, db, cipher)
    }

    async fn add_connection(db: &Db, cipher: &Arc<CredentialCipher>, name: &str) {
        ConnectionRepository::new(db.pool.clone(), cipher.clone())
            .create(CreateConnection {
                name: name.to_string(),
                provider_type: "openai-compatible".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                api_key: None,
                custom_headers: Default::default(),
                enabled: true,
                connect_timeout_ms: None,
                idle_timeout_ms: None,
                pricing_model: None,
                cache_retention: None,
                provider_id: None,
            })
            .await
            .expect("create connection");
    }

    #[tokio::test]
    async fn snapshot_is_shared_until_invalidated() {
        let (cache, db, cipher) = cache(60_000).await;
        let first = cache.snapshot().await.expect("first");
        let second = cache.snapshot().await.expect("second");

        assert!(
            Arc::ptr_eq(&first.catalog, &second.catalog),
            "within the TTL the same snapshot must be reused"
        );

        add_connection(&db, &cipher, "late").await;
        let still_cached = cache.snapshot().await.expect("cached");
        assert!(
            Arc::ptr_eq(&first.catalog, &still_cached.catalog),
            "admin writes must invalidate explicitly; until then the cache is reused"
        );

        cache.invalidate().await;
        let refreshed = cache.snapshot().await.expect("refreshed");
        assert!(!Arc::ptr_eq(&first.catalog, &refreshed.catalog));
        assert_eq!(refreshed.catalog.connections.len(), 1);
    }

    #[tokio::test]
    async fn ttl_expiry_reloads() {
        let (cache, db, cipher) = cache(1).await;
        let first = cache.snapshot().await.expect("first");

        add_connection(&db, &cipher, "late").await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let refreshed = cache.snapshot().await.expect("refreshed");
        assert!(!Arc::ptr_eq(&first.catalog, &refreshed.catalog));
        assert_eq!(refreshed.catalog.connections.len(), 1);
    }

    #[tokio::test]
    async fn zero_ttl_caches_until_invalidated() {
        let (cache, db, cipher) = cache(0).await;
        let first = cache.snapshot().await.expect("first");

        add_connection(&db, &cipher, "late").await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let cached = cache.snapshot().await.expect("cached");
        assert!(Arc::ptr_eq(&first.catalog, &cached.catalog));
    }

    #[tokio::test]
    async fn apply_invalidates_and_reloads() {
        let (cache, db, cipher) = cache(0).await;
        let first = cache.snapshot().await.expect("first");

        add_connection(&db, &cipher, "late").await;
        cache.apply(&config(60_000)).await;

        let refreshed = cache.snapshot().await.expect("refreshed");
        assert!(!Arc::ptr_eq(&first.catalog, &refreshed.catalog));
        assert_eq!(refreshed.catalog.connections.len(), 1);
    }
}
