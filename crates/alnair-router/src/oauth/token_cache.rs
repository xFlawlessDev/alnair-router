//! Access-token cache with single-flight refresh.
//!
//! Shaped like `PricingCache`: an `RwLock` map for the hot read path, loaded on
//! first use. The refresh itself is guarded per account, so N concurrent
//! requests for one account make **one** call to the token endpoint and the
//! rest await that result rather than stampeding the provider.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, RwLock};

use crate::crypto::CredentialCipher;
use crate::db::repos::oauth_accounts::OAuthAccountRepository;
use crate::error::{Error, Result};
use crate::oauth::credential::OAuthCredential;
use crate::oauth::flows;

/// How long before expiry a token is renewed, so a request never carries one
/// that dies in flight.
const REFRESH_LEAD: ChronoDuration = ChronoDuration::minutes(5);

/// A cached access token and the moment it stops being usable.
#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: Option<chrono::DateTime<Utc>>,
}

/// Where the repository comes from, so the cache does not hold `AppState`.
pub struct OAuthTokenCache {
    pool: SqlitePool,
    cipher: Arc<CredentialCipher>,
    tokens: RwLock<HashMap<String, CachedToken>>,
    /// One lock per account, held across the refresh call.
    refreshes: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Renewal window, overridable in tests.
    lead: ChronoDuration,
}

impl OAuthTokenCache {
    pub fn new(pool: SqlitePool, cipher: Arc<CredentialCipher>) -> Self {
        Self {
            pool,
            cipher,
            tokens: RwLock::new(HashMap::new()),
            refreshes: Mutex::new(HashMap::new()),
            lead: REFRESH_LEAD,
        }
    }

    /// Returns a usable access token for `account_id`, refreshing it first when
    /// it is missing or close to expiry.
    pub async fn access_token(&self, account_id: &str) -> Result<String> {
        let repository = self.repository();

        let account = repository
            .find(account_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("oauth account '{account_id}' not found")))?;

        if !account.is_enabled() {
            return Err(Error::BadRequest(format!(
                "oauth account '{}' is disabled",
                account.label
            )));
        }

        // Fast path: a still-valid token needs no locking beyond the read.
        if let Some(token) = self.cached(account_id).await {
            return Ok(token);
        }

        let gate = self.gate_for(account_id).await;
        let _held = gate.lock().await;

        // Another request may have refreshed while this one waited for the gate.
        if let Some(token) = self.cached(account_id).await {
            return Ok(token);
        }

        let mut credential = account.credential().clone();
        if credential.needs_refresh(Utc::now(), self.lead) {
            self.renew(account_id, &mut credential).await?;
        }

        let token = credential.access_token.clone();
        self.remember(account_id, &credential).await;
        Ok(token)
    }

    /// Renews the credential and persists the rotated blob.
    async fn renew(&self, account_id: &str, credential: &mut OAuthCredential) -> Result<()> {
        let Some(refresh_token) = credential.refresh_token.clone() else {
            return Err(Error::BadRequest(
                "this account has no refresh token; sign in again to reconnect it".to_string(),
            ));
        };

        let renewed = flows::refresh(&credential.endpoints, &refresh_token).await?;
        credential.apply(renewed, Utc::now());

        // A rotated refresh token must survive a restart, so it is written back
        // before the token is handed out.
        self.repository()
            .store_credential(account_id, credential)
            .await?;

        tracing::info!(account = %account_id, "refreshed an OAuth access token");
        Ok(())
    }

    /// The cached token, when it is still comfortably valid.
    async fn cached(&self, account_id: &str) -> Option<String> {
        let tokens = self.tokens.read().await;
        let entry = tokens.get(account_id)?;
        let usable = match entry.expires_at {
            Some(expiry) => expiry - self.lead > Utc::now(),
            // No known expiry: trust it until a request proves otherwise.
            None => true,
        };
        usable.then(|| entry.access_token.clone())
    }

    async fn remember(&self, account_id: &str, credential: &OAuthCredential) {
        self.tokens.write().await.insert(
            account_id.to_string(),
            CachedToken {
                access_token: credential.access_token.clone(),
                expires_at: credential.expires_at,
            },
        );
    }

    /// The per-account refresh gate, created on first use.
    async fn gate_for(&self, account_id: &str) -> Arc<Mutex<()>> {
        let mut gates = self.refreshes.lock().await;
        gates
            .entry(account_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Drops a cached token, so the next request re-reads the account.
    pub async fn invalidate(&self, account_id: &str) {
        self.tokens.write().await.remove(account_id);
    }

    fn repository(&self) -> OAuthAccountRepository {
        OAuthAccountRepository::new(self.pool.clone(), self.cipher.clone())
    }
}

/// The renewal window, exposed so a test can assert the default.
#[cfg(test)]
pub fn refresh_lead() -> ChronoDuration {
    REFRESH_LEAD
}
