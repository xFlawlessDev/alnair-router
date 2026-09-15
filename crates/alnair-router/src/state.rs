//! Shared application state for the HTTP layer.

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::RouterConfig;
use crate::crypto::CredentialCipher;
use crate::db::Db;
use crate::db::repos::aliases::AliasRepository;
use crate::db::repos::api_keys::ApiKeyRepository;
use crate::db::repos::combos::ComboRepository;
use crate::db::repos::connections::ConnectionRepository;
use crate::db::repos::key_plans::KeyPlanRepository;
use crate::db::repos::usage::UsageRepository;
use crate::error::Result;
use crate::limits::{RateLimiter, UpstreamLimiter};
use crate::metrics::Metrics;
use crate::pricing::{PricingCache, PricingRepository};
use crate::telemetry::ActivityTracker;
use crate::upstream::chat_backend::{ProviderRegistry, RetryPolicy};
use crate::upstream::{Executor, ExecutorSettings, UpstreamTimeouts};

/// State shared by every handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RouterConfig>,
    pub pool: SqlitePool,
    pub cipher: Arc<CredentialCipher>,
    pub executor: Executor,
    pub limiter: UpstreamLimiter,
    pub rate_limiter: RateLimiter,
    pub metrics: Arc<Metrics>,
    pub telemetry: Arc<ActivityTracker>,
    pub pricing_cache: Arc<PricingCache>,
    catalog_cache: Arc<crate::model::CatalogCache>,
}

impl AppState {
    /// Builds state from an open database and the shared provider registry.
    ///
    /// Fails when `secrets.key` is malformed; `config::load` validates this up
    /// front, so the error only surfaces for callers that skip validation.
    pub fn new(config: RouterConfig, db: Db) -> Result<Self> {
        let cipher = Arc::new(CredentialCipher::from_config(&config.secrets)?);
        let retry = RetryPolicy {
            max_retries_per_tier: config.router.max_retries_per_tier,
            max_retry_delay_ms: config.router.max_retry_delay_ms,
        };
        let limiter = UpstreamLimiter::new(&config.limits);
        let rate_limiter = RateLimiter::new(&config.rate_limit);
        let timeouts = UpstreamTimeouts::from_config(&config);
        let metrics = Arc::new(Metrics::default());
        let telemetry = Arc::new(ActivityTracker::new());
        let catalog_cache = Arc::new(crate::model::CatalogCache::new(
            &config,
            db.pool.clone(),
            cipher.clone(),
        ));
        let pricing_cache = Arc::new(PricingCache::new(db.pool.clone()));

        Ok(Self {
            config: Arc::new(config),
            pool: db.pool.clone(),
            cipher,
            executor: Executor::with_settings(
                Arc::new(ProviderRegistry::with_defaults()),
                ExecutorSettings {
                    retry,
                    limiter: limiter.clone(),
                    timeouts,
                    metrics: metrics.clone(),
                    telemetry: telemetry.clone(),
                    pricing: Some(pricing_cache.clone()),
                },
            ),
            limiter,
            rate_limiter,
            metrics,
            telemetry,
            pricing_cache,
            catalog_cache,
        })
    }

    pub fn connections(&self) -> ConnectionRepository {
        ConnectionRepository::new(self.pool.clone(), self.cipher.clone())
    }

    pub fn aliases(&self) -> AliasRepository {
        AliasRepository::new(self.pool.clone())
    }

    pub fn combos(&self) -> ComboRepository {
        ComboRepository::new(self.pool.clone())
    }

    pub fn api_keys(&self) -> ApiKeyRepository {
        ApiKeyRepository::new(self.pool.clone())
    }

    pub fn key_plans(&self) -> KeyPlanRepository {
        KeyPlanRepository::new(self.pool.clone())
    }

    pub fn pricing(&self) -> PricingRepository {
        PricingRepository::new(self.pool.clone())
    }

    pub fn usage(&self) -> UsageRepository {
        UsageRepository::new(self.pool.clone())
    }

    /// Loads a routing snapshot and builds a resolver over it.
    ///
    /// The snapshot is cached; admin writes invalidate it explicitly.
    pub async fn resolver(&self) -> Result<Arc<crate::model::Resolver>> {
        Ok(self.catalog_snapshot().await?.resolver)
    }

    /// Returns the cached catalog snapshot, reloading when stale.
    pub async fn catalog_snapshot(&self) -> Result<crate::model::CatalogSnapshot> {
        self.catalog_cache.snapshot().await
    }

    /// Drops the cached routing catalog; called by every admin mutation.
    pub async fn invalidate_catalog(&self) {
        self.catalog_cache.invalidate().await;
    }
}
