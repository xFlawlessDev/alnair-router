//! Shared application state for the HTTP layer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use sqlx::SqlitePool;
use tokio::sync::Notify;

use crate::auth::{AuthRepository, generate_setup_code};
use crate::config::RouterConfig;
use crate::crypto::CredentialCipher;
use crate::db::Db;
use crate::db::repos::aliases::AliasRepository;
use crate::db::repos::api_keys::ApiKeyRepository;
use crate::db::repos::combos::ComboRepository;
use crate::db::repos::connection_accounts::ConnectionAccountRepository;
use crate::db::repos::connections::ConnectionRepository;
use crate::db::repos::key_plans::KeyPlanRepository;
use crate::db::repos::usage::UsageRepository;
use crate::error::Result;
use crate::limits::{RateLimiter, UpstreamLimiter};
use crate::metrics::Metrics;
use crate::pricing::{PricingCache, PricingRepository};
use crate::settings::{SettingsOverrides, SettingsRepository};
use crate::telemetry::ActivityTracker;
use crate::upstream::chat_backend::{ProviderRegistry, RetryPolicy};
use crate::upstream::{Executor, ExecutorSettings, KeyRotator, UpstreamTimeouts};

/// State shared by every handler.
#[derive(Clone)]
pub struct AppState {
    /// Effective configuration: `base_config` plus dashboard overrides.
    pub(crate) config: Arc<RwLock<RouterConfig>>,
    /// Configuration as loaded from file/env, before dashboard overrides. A
    /// reset drops every override and rebuilds the effective config from it.
    base_config: Arc<RouterConfig>,
    pub pool: SqlitePool,
    pub cipher: Arc<CredentialCipher>,
    pub executor: Executor,
    pub limiter: UpstreamLimiter,
    pub rate_limiter: RateLimiter,
    pub metrics: Arc<Metrics>,
    pub telemetry: Arc<ActivityTracker>,
    pub pricing_cache: Arc<PricingCache>,
    /// Wakes the pricing sync loop after a settings save.
    pub pricing_sync_trigger: Arc<Notify>,
    /// Wakes the serve loop to rebind when LAN access or the port changes.
    pub rebind: Arc<Notify>,
    /// Whether the serve and pricing loops are running; see [`Self::start_loops`].
    loops_started: Arc<AtomicBool>,
    /// One-time code for the first dashboard password; `None` once set.
    setup_code: Arc<Mutex<Option<String>>>,
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
            config: Arc::new(RwLock::new(config.clone())),
            base_config: Arc::new(config),
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
                    key_rotator: KeyRotator::default(),
                },
            ),
            limiter,
            rate_limiter,
            metrics,
            telemetry,
            pricing_cache,
            pricing_sync_trigger: Arc::new(Notify::new()),
            rebind: Arc::new(Notify::new()),
            loops_started: Arc::new(AtomicBool::new(false)),
            setup_code: Arc::new(Mutex::new(None)),
            catalog_cache,
        })
    }

    /// Snapshot of the effective configuration (file/env plus overrides).
    pub fn config_snapshot(&self) -> RouterConfig {
        self.config.read().expect("config lock poisoned").clone()
    }

    /// Configuration as loaded from file/env, before dashboard overrides.
    pub fn base_config(&self) -> &RouterConfig {
        self.base_config.as_ref()
    }

    /// Returns the configuration `overrides` would produce, validating it
    /// without applying anything. Used to reject bad saves before persisting.
    pub fn merged_config(&self, overrides: &SettingsOverrides) -> Result<RouterConfig> {
        overrides.merge(self.base_config())
    }

    /// Recomputes the effective configuration from `overrides` and pushes it
    /// into the live components. Persisting the overrides is the caller's job.
    pub async fn apply_overrides(&self, overrides: &SettingsOverrides) -> Result<RouterConfig> {
        let next = self.merged_config(overrides)?;

        self.limiter.apply(&next.limits);
        self.rate_limiter.apply(&next.rate_limit);
        self.executor.apply(&next);
        self.catalog_cache.apply(&next).await;

        let (pricing_changed, rebind) = {
            let current = self.config.read().expect("config lock poisoned");
            (
                current.pricing.sync_enabled != next.pricing.sync_enabled
                    || current.pricing.sync_interval_secs != next.pricing.sync_interval_secs
                    || current.pricing.source_url != next.pricing.source_url,
                current.server.listen_address() != next.server.listen_address(),
            )
        };

        *self.config.write().expect("config lock poisoned") = next.clone();

        if self.loops_started.load(Ordering::SeqCst) {
            if pricing_changed {
                self.pricing_sync_trigger.notify_one();
            }
            if rebind {
                self.rebind.notify_one();
            }
        }

        Ok(next)
    }

    /// Marks the serve and pricing loops as running, so later configuration
    /// changes may wake them.
    ///
    /// Startup applies the stored dashboard overrides through the same path as
    /// a dashboard save, but no loop exists yet. A `Notify` permit handed out
    /// then is not dropped: the loop would consume it the moment it first
    /// waits, so the listener would rebind (and log) for nothing.
    pub fn start_loops(&self) {
        self.loops_started.store(true, Ordering::SeqCst);
    }

    /// Dashboard password and sessions.
    pub fn auth(&self) -> AuthRepository {
        AuthRepository::new(self.pool.clone())
    }

    /// Generates and stores the first-run setup code when no password exists.
    /// Returns the code so the caller can log it.
    pub async fn init_setup_code(&self) -> Result<Option<String>> {
        if self.auth().password_set().await? {
            return Ok(None);
        }

        let code = generate_setup_code();
        *self.setup_code.lock().expect("setup code poisoned") = Some(code.clone());
        Ok(Some(code))
    }

    pub fn setup_code(&self) -> Option<String> {
        self.setup_code.lock().expect("setup code poisoned").clone()
    }

    pub fn clear_setup_code(&self) {
        *self.setup_code.lock().expect("setup code poisoned") = None;
    }

    pub fn connections(&self) -> ConnectionRepository {
        ConnectionRepository::new(self.pool.clone(), self.cipher.clone())
    }

    /// Extra API keys attached to connections.
    pub fn connection_accounts(&self) -> ConnectionAccountRepository {
        ConnectionAccountRepository::new(self.pool.clone(), self.cipher.clone())
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

    /// Dashboard-managed setting overrides.
    pub fn settings(&self) -> SettingsRepository {
        SettingsRepository::new(self.pool.clone())
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
