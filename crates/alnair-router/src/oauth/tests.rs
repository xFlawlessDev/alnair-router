//! Tests for the OAuth module: PKCE correctness, the login registry, and the
//! credential document's renewal semantics.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};

use super::*;

const UNRESERVED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

fn credential(expires_at: Option<chrono::DateTime<Utc>>) -> OAuthCredential {
    OAuthCredential {
        access_token: "access-1".to_string(),
        refresh_token: Some("refresh-1".to_string()),
        expires_at,
        token_type: Some("Bearer".to_string()),
        scope: None,
        endpoints: EndpointConfig {
            client_id: "client".to_string(),
            client_secret: None,
            authorize_url: "https://example.invalid/authorize".to_string(),
            token_url: "https://example.invalid/token".to_string(),
            device_code_url: None,
            user_info_url: None,
            scopes: "read".to_string(),
        },
        account: None,
    }
}

#[test]
fn the_verifier_is_a_legal_pkce_verifier() {
    let verifier = pkce::code_verifier();

    assert!(
        (43..=128).contains(&verifier.len()),
        "verifier length {} is outside RFC 7636's range",
        verifier.len()
    );
    assert!(
        verifier.chars().all(|c| UNRESERVED.contains(c)),
        "verifier uses characters outside the unreserved set"
    );
}

#[test]
fn the_challenge_is_the_unpadded_s256_of_the_verifier() {
    let verifier = pkce::code_verifier();
    let challenge = pkce::code_challenge(&verifier);

    // Computed independently of the implementation under test.
    let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    assert_eq!(challenge, expected);
    assert!(!challenge.contains('='), "the challenge must be unpadded");
    assert!(!challenge.contains('+') && !challenge.contains('/'));
}

#[test]
fn nonces_are_distinct_across_calls() {
    let verifiers: std::collections::HashSet<_> = (0..64).map(|_| pkce::code_verifier()).collect();
    assert_eq!(verifiers.len(), 64, "a code verifier repeated");

    let states: std::collections::HashSet<_> = (0..64).map(|_| pkce::random_state()).collect();
    assert_eq!(states.len(), 64, "a state nonce repeated");

    let ids: std::collections::HashSet<_> = (0..64).map(|_| pkce::random_id()).collect();
    assert_eq!(ids.len(), 64, "a login id repeated");
}

#[test]
fn a_credential_without_an_expiry_is_never_refreshed_on_a_timer() {
    let now = Utc::now();
    assert!(!credential(None).needs_refresh(now, Duration::minutes(5)));
}

#[test]
fn a_credential_is_refreshed_inside_the_lead_window() {
    let now = Utc::now();

    // Expires in a minute, well inside the five-minute lead.
    assert!(credential(Some(now + Duration::minutes(1))).needs_refresh(now, Duration::minutes(5)));
    // Expires in an hour, comfortably outside it.
    assert!(!credential(Some(now + Duration::hours(1))).needs_refresh(now, Duration::minutes(5)));
    // Already expired.
    assert!(credential(Some(now - Duration::minutes(1))).needs_refresh(now, Duration::minutes(5)));
}

#[test]
fn applying_a_renewal_keeps_the_endpoints_and_rotates_the_tokens() {
    let now = Utc::now();
    let mut existing = credential(None);
    let original_endpoints = existing.endpoints.clone();

    existing.apply(
        flows::TokenResponse {
            access_token: "access-2".to_string(),
            refresh_token: Some("refresh-2".to_string()),
            expires_in: Some(3600),
            token_type: Some("Bearer".to_string()),
            scope: None,
        },
        now,
    );

    assert_eq!(existing.access_token, "access-2");
    assert_eq!(existing.refresh_token.as_deref(), Some("refresh-2"));
    assert_eq!(existing.expires_at, Some(now + Duration::seconds(3600)));
    assert_eq!(existing.endpoints, original_endpoints);
}

#[test]
fn a_renewal_that_omits_the_refresh_token_keeps_the_existing_one() {
    let now = Utc::now();
    let mut existing = credential(None);

    existing.apply(
        flows::TokenResponse {
            access_token: "access-2".to_string(),
            refresh_token: None,
            expires_in: None,
            token_type: None,
            scope: None,
        },
        now,
    );

    assert_eq!(existing.access_token, "access-2");
    assert_eq!(existing.refresh_token.as_deref(), Some("refresh-1"));
}

#[tokio::test]
async fn a_pending_login_is_found_by_state_only_while_it_lasts() {
    let registry = LoginRegistry::new();
    let login = PendingLogin::pkce(
        "login-1".to_string(),
        "connection-1".to_string(),
        "work".to_string(),
        "gitlab-duo".to_string(),
        credential(None).endpoints,
        "state-1".to_string(),
        "verifier-1".to_string(),
        "http://127.0.0.1:7878/api/oauth/callback".to_string(),
    );

    registry.insert(login).await;

    let found = registry
        .by_state("state-1")
        .await
        .expect("login found by state");
    assert_eq!(found.id, "login-1");
    assert_eq!(found.verifier, "verifier-1");

    assert!(registry.by_state("state-unknown").await.is_none());

    let view = registry.view("login-1").await.expect("view");
    assert_eq!(view.status, LoginState::Pending);

    registry
        .settle(
            "login-1",
            LoginState::Completed {
                account_id: "account-1".to_string(),
            },
        )
        .await;
    let view = registry.view("login-1").await.expect("view");
    assert_eq!(view.status.as_str(), "completed");

    assert!(registry.remove("login-1").await);
    assert!(registry.view("login-1").await.is_none());
}

#[tokio::test]
async fn a_device_login_carries_its_challenge() {
    let registry = LoginRegistry::new();
    let login = PendingLogin::device(
        "login-2".to_string(),
        "connection-1".to_string(),
        "work".to_string(),
        "google".to_string(),
        credential(None).endpoints,
        DeviceChallenge {
            user_code: "ABCD-EFGH".to_string(),
            verification_uri: Some("https://example.invalid/device".to_string()),
            expires_in: Some(300),
        },
    );

    registry.insert(login).await;

    let view = registry.view("login-2").await.expect("view");
    let device = view.device.expect("device challenge");
    assert_eq!(device.user_code, "ABCD-EFGH");
    assert_eq!(view.status, LoginState::Pending);
    // A device login has no PKCE state to be found by.
    assert!(registry.by_state("").await.is_none());
}

#[test]
fn the_generic_preset_is_the_fallback_and_ships_no_identity() {
    let generic = presets::find("does-not-exist");
    assert_eq!(generic.id, "generic");
    assert!(generic.authorize_url.is_empty());
    assert!(generic.token_url.is_empty());

    // No preset may ship a client identity.
    for preset in presets::presets() {
        let serialized = serde_json::to_string(&preset).expect("serialize");
        assert!(
            !serialized.contains("client_id") && !serialized.contains("client_secret"),
            "preset {} appears to ship client credentials",
            preset.id
        );
    }
}

#[test]
fn client_inputs_are_validated_and_trimmed() {
    let endpoints = ClientInputs {
        client_id: "  client-1  ".to_string(),
        client_secret: Some("   ".to_string()),
        authorize_url: " https://example.invalid/authorize ".to_string(),
        token_url: "https://example.invalid/token".to_string(),
        device_code_url: None,
        user_info_url: Some("https://example.invalid/userinfo".to_string()),
        scopes: "  read write  ".to_string(),
    }
    .into_endpoints()
    .expect("valid");

    assert_eq!(endpoints.client_id, "client-1");
    // A whitespace-only secret is treated as absent, not stored as blank.
    assert_eq!(endpoints.client_secret, None);
    assert_eq!(endpoints.scopes, "read write");

    let missing_id = ClientInputs {
        client_id: "   ".to_string(),
        client_secret: None,
        authorize_url: "https://example.invalid/authorize".to_string(),
        token_url: "https://example.invalid/token".to_string(),
        device_code_url: None,
        user_info_url: None,
        scopes: String::new(),
    };
    assert!(missing_id.into_endpoints().is_err());

    // The token endpoint is always required — it is where refreshes go.
    let missing_token_url = ClientInputs {
        client_id: "client-1".to_string(),
        client_secret: None,
        authorize_url: "https://example.invalid/authorize".to_string(),
        token_url: String::new(),
        device_code_url: None,
        user_info_url: None,
        scopes: String::new(),
    };
    assert!(missing_token_url.into_endpoints().is_err());

    // A device-only provider has no authorize URL and must still validate; the
    // login handler is what rejects a *browser* login without one.
    let device_only = ClientInputs {
        client_id: "client-1".to_string(),
        client_secret: None,
        authorize_url: String::new(),
        token_url: "https://example.invalid/token".to_string(),
        device_code_url: Some("https://example.invalid/device".to_string()),
        user_info_url: None,
        scopes: String::new(),
    }
    .into_endpoints()
    .expect("a device-only provider needs no authorize URL");
    assert!(device_only.authorize_url.is_empty());
    assert_eq!(
        device_only.device_code_url.as_deref(),
        Some("https://example.invalid/device")
    );
}

#[test]
fn the_redirect_uri_is_the_router_own_callback_path() {
    assert_eq!(
        redirect_uri("127.0.0.1:7878"),
        "http://127.0.0.1:7878/api/oauth/callback"
    );
}

#[test]
fn token_cache_uses_a_five_minute_lead() {
    assert_eq!(token_cache::refresh_lead().num_minutes(), 5);
}

/// A token cache over a fresh in-memory database with one seeded account.
async fn cache_with_account(
    credential: OAuthCredential,
    enabled: bool,
) -> (
    std::sync::Arc<OAuthTokenCache>,
    crate::db::Db,
    String,
    String,
) {
    use crate::config::SecretsConfig;
    use crate::crypto::CredentialCipher;
    use crate::db::repos::connections::{ConnectionRepository, CreateConnection};
    use crate::db::repos::oauth_accounts::{CreateOAuthAccount, OAuthAccountRepository};

    const SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    let db = crate::db::Db::connect_in_memory().await.expect("db");
    let cipher = std::sync::Arc::new(
        CredentialCipher::from_config(&SecretsConfig {
            key: Some(SECRET.to_string()),
        })
        .expect("cipher"),
    );

    let connection = ConnectionRepository::new(db.pool.clone(), cipher.clone())
        .create(CreateConnection {
            name: "gitlab-duo".to_string(),
            provider_type: "anthropic-native".to_string(),
            base_url: "https://gitlab.com/api/v4".to_string(),
            api_key: None,
            custom_headers: Default::default(),
            enabled: true,
            connect_timeout_ms: None,
            idle_timeout_ms: None,
            pricing_model: None,
            cache_retention: None,
            auth_style: Some("bearer".to_string()),
            provider_id: None,
        })
        .await
        .expect("connection");

    let account = OAuthAccountRepository::new(db.pool.clone(), cipher.clone())
        .create(
            &connection.id,
            CreateOAuthAccount {
                label: "work".to_string(),
                provider_key: "gitlab-duo".to_string(),
                credential,
                enabled,
            },
        )
        .await
        .expect("account");

    let cache = std::sync::Arc::new(OAuthTokenCache::new(db.pool.clone(), cipher));
    (cache, db, account.id, connection.id)
}

#[tokio::test]
async fn a_token_without_an_expiry_is_served_from_cache() {
    // No expiry means no timer-driven refresh, so this never reaches the
    // network — the token endpoint is a genuinely unreachable host.
    let (cache, _db, account_id, _) = cache_with_account(credential(None), true).await;

    let token = cache.access_token(&account_id).await.expect("token");
    assert_eq!(token, "access-1");

    // A second read is served from the cache, not re-read from storage.
    let again = cache.access_token(&account_id).await.expect("token");
    assert_eq!(again, "access-1");
}

#[tokio::test]
async fn a_disabled_account_is_refused_before_any_network_call() {
    let (cache, _db, account_id, _) = cache_with_account(credential(None), false).await;

    let error = cache
        .access_token(&account_id)
        .await
        .expect_err("disabled account");
    assert!(
        matches!(error, crate::error::Error::BadRequest(_)),
        "{error:?}"
    );
    assert!(error.to_string().contains("disabled"), "{error}");
}

#[tokio::test]
async fn an_unknown_account_is_reported_as_not_found() {
    let (cache, _db, _account_id, _) = cache_with_account(credential(None), true).await;

    let error = cache
        .access_token("no-such-account")
        .await
        .expect_err("unknown account");
    assert!(
        matches!(error, crate::error::Error::NotFound(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_expired_token_without_a_refresh_token_fails_fast() {
    // Expiry inside the lead window forces a refresh attempt; with no refresh
    // token there is nothing to send, so it must fail without a request.
    let mut expired = credential(Some(Utc::now() - Duration::minutes(1)));
    expired.refresh_token = None;
    let (cache, _db, account_id, _) = cache_with_account(expired, true).await;

    let error = cache
        .access_token(&account_id)
        .await
        .expect_err("no refresh token");
    assert!(
        matches!(error, crate::error::Error::BadRequest(_)),
        "{error:?}"
    );
    assert!(error.to_string().contains("refresh token"), "{error}");
}

#[tokio::test]
async fn invalidating_drops_the_cached_token() {
    let (cache, _db, account_id, _) = cache_with_account(credential(None), true).await;

    assert_eq!(
        cache.access_token(&account_id).await.expect("token"),
        "access-1"
    );
    cache.invalidate(&account_id).await;
    // Still resolvable: invalidation forces a re-read, not a failure.
    assert_eq!(
        cache.access_token(&account_id).await.expect("token"),
        "access-1"
    );
}
