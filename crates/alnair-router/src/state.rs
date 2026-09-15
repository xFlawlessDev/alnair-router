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
use crate::db::repos::usage::UsageRepository;
use crate::error::Result;
use crate::limits::{RateLimiter, UpstreamLimiter};
use crate::upstream::Executor;
use crate::upstream::chat_backend::{ProviderRegistry, RetryPolicy};

/// State shared by every handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RouterConfig>,
    pub pool: SqlitePool,
    pub cipher: Arc<CredentialCipher>,
    pub executor: Executor,
    pub limiter: UpstreamLimiter,
    pub rate_limiter: RateLimiter,
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

        Ok(Self {
            config: Arc::new(config),
            pool: db.pool.clone(),
            cipher,
            executor: Executor::with_settings(
                Arc::new(ProviderRegistry::with_defaults()),
                retry,
                limiter.clone(),
            ),
            limiter,
            rate_limiter,
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

    pub fn usage(&self) -> UsageRepository {
        UsageRepository::new(self.pool.clone())
    }

    /// Loads a routing snapshot and builds a resolver over it.
    pub async fn resolver(&self) -> Result<crate::model::Resolver> {
        let catalog = crate::model::Catalog::load(&self.pool, &self.cipher).await?;
        Ok(catalog.resolver(
            self.config.router.default_connection.clone(),
            self.config.router.max_attempts,
        ))
    }
}
