//! Shared application state for the HTTP layer.

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::RouterConfig;
use crate::db::Db;
use crate::db::repos::aliases::AliasRepository;
use crate::db::repos::api_keys::ApiKeyRepository;
use crate::db::repos::combos::ComboRepository;
use crate::db::repos::connections::ConnectionRepository;
use crate::db::repos::usage::UsageRepository;
use crate::upstream::Executor;
use crate::upstream::chat_backend::ProviderRegistry;

/// State shared by every handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RouterConfig>,
    pub pool: SqlitePool,
    pub executor: Executor,
}

impl AppState {
    /// Builds state from an open database and the shared provider registry.
    pub fn new(config: RouterConfig, db: Db) -> Self {
        Self {
            config: Arc::new(config),
            pool: db.pool.clone(),
            executor: Executor::new(Arc::new(ProviderRegistry::with_defaults())),
        }
    }

    pub fn connections(&self) -> ConnectionRepository {
        ConnectionRepository::new(self.pool.clone())
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
    pub async fn resolver(&self) -> crate::error::Result<crate::model::Resolver> {
        let catalog = crate::model::Catalog::load(&self.pool).await?;
        Ok(catalog.resolver(
            self.config.router.default_connection.clone(),
            self.config.router.max_attempts,
        ))
    }
}
