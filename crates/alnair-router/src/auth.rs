//! Single-owner admin password authentication.
//!
//! The dashboard signs in with a password only (no username). Sessions are
//! opaque access/refresh token pairs stored as SHA-256 hashes; refreshing
//! rotates the refresh token, and presenting an already-rotated token revokes
//! the whole family — the theft signal. `server.admin_token` remains supported
//! for scripts and CI.

use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use crate::error::{Error, Result};

/// Access tokens live this long before `/api/auth/refresh` must rotate them.
pub const ACCESS_TTL_MINUTES: i64 = 30;
/// Sliding refresh lifetime; every rotation extends it.
pub const REFRESH_TTL_DAYS: i64 = 7;
/// Hard cap from sign-in, however often the refresh token rotates.
pub const ABSOLUTE_TTL_DAYS: i64 = 30;

/// One signed-in browser session, handed to the SPA on login and refresh.
#[derive(Debug, Clone, Serialize)]
pub struct AuthSession {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: DateTime<Utc>,
    pub refresh_expires_at: DateTime<Utc>,
}

/// Outcome of rotating a refresh token.
pub enum Rotation {
    Rotated(AuthSession),
    /// The token was already rotated: a copy is in use, so the family is dead.
    Reused,
    /// Unknown, revoked or expired beyond recovery.
    Missing,
}

#[derive(Debug, FromRow)]
struct SessionRow {
    id: String,
    kind: String,
    family_id: String,
    expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    rotated_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

/// Password and session persistence.
pub struct AuthRepository {
    pool: SqlitePool,
}

impl AuthRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// True once a dashboard password exists.
    pub async fn password_set(&self) -> Result<bool> {
        Ok(self.password_hash().await?.is_some())
    }

    pub async fn password_hash(&self) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT password_hash FROM auth WHERE id = 1")
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(hash,)| hash))
    }

    /// Stores (or replaces) the password and revokes every existing session.
    pub async fn set_password(&self, password: &str) -> Result<()> {
        let hash = hash_password(password)?;
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO auth (id, password_hash, created_at, updated_at)
             VALUES (1, ?1, ?2, ?2)
             ON CONFLICT (id) DO UPDATE SET password_hash = excluded.password_hash,
                                            updated_at = excluded.updated_at",
        )
        .bind(&hash)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.revoke_all().await
    }

    /// Verifies the password and opens a new session family.
    pub async fn login(&self, password: &str) -> Result<AuthSession> {
        let Some(hash) = self.password_hash().await? else {
            return Err(Error::Unauthorized(
                "no dashboard password is set yet".to_string(),
            ));
        };
        if !verify_password(&hash, password) {
            return Err(Error::Unauthorized("invalid password".to_string()));
        }

        let family_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let absolute = now + Duration::days(ABSOLUTE_TTL_DAYS);
        let session = self.new_tokens(&family_id, now, absolute).await?;
        Ok(session)
    }

    /// True when the access token matches a live session.
    pub async fn authenticate(&self, access_token: &str) -> Result<bool> {
        let row = self.find_token(&hash_token(access_token)).await?;
        Ok(row.is_some_and(|row| row.kind == "access" && live(&row, Utc::now())))
    }

    /// Rotates a refresh token. Reuse of a rotated token revokes the family.
    pub async fn refresh(&self, refresh_token: &str) -> Result<Rotation> {
        let Some(row) = self.find_token(&hash_token(refresh_token)).await? else {
            return Ok(Rotation::Missing);
        };
        if row.kind != "refresh" || row.revoked_at.is_some() || !live(&row, Utc::now()) {
            return Ok(Rotation::Missing);
        }
        if row.rotated_at.is_some() {
            self.revoke_family(&row.family_id).await?;
            return Ok(Rotation::Reused);
        }

        let now = Utc::now();
        let mut transaction = self.pool.begin().await?;
        sqlx::query("UPDATE auth_sessions SET rotated_at = ?1 WHERE id = ?2")
            .bind(now)
            .bind(&row.id)
            .execute(&mut *transaction)
            .await?;

        let access = generate_token();
        let refresh = generate_token();
        let access_expires = now + Duration::minutes(ACCESS_TTL_MINUTES);
        let refresh_expires = refresh_expiry(now, row.absolute_expires_at);

        insert_session(
            &mut transaction,
            &row.family_id,
            "access",
            &hash_token(&access),
            now,
            access_expires,
            row.absolute_expires_at,
        )
        .await?;
        insert_session(
            &mut transaction,
            &row.family_id,
            "refresh",
            &hash_token(&refresh),
            now,
            refresh_expires,
            row.absolute_expires_at,
        )
        .await?;
        transaction.commit().await?;

        Ok(Rotation::Rotated(AuthSession {
            access_token: access,
            refresh_token: refresh,
            access_expires_at: access_expires,
            refresh_expires_at: refresh_expires,
        }))
    }

    /// Revokes the family the access token belongs to (logout).
    pub async fn revoke_family_for_access(&self, access_token: &str) -> Result<()> {
        if let Some(row) = self.find_token(&hash_token(access_token)).await? {
            self.revoke_family(&row.family_id).await?;
        }
        Ok(())
    }

    pub async fn revoke_family(&self, family_id: &str) -> Result<()> {
        sqlx::query("UPDATE auth_sessions SET revoked_at = ?1 WHERE family_id = ?2")
            .bind(Utc::now())
            .bind(family_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn revoke_all(&self) -> Result<()> {
        sqlx::query("UPDATE auth_sessions SET revoked_at = ?1 WHERE revoked_at IS NULL")
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn find_token(&self, token_hash: &str) -> Result<Option<SessionRow>> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, kind, family_id, expires_at, absolute_expires_at, rotated_at, revoked_at
             FROM auth_sessions WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn new_tokens(
        &self,
        family_id: &str,
        now: DateTime<Utc>,
        absolute: DateTime<Utc>,
    ) -> Result<AuthSession> {
        let access = generate_token();
        let refresh = generate_token();
        let access_expires = now + Duration::minutes(ACCESS_TTL_MINUTES);
        let refresh_expires = refresh_expiry(now, absolute);

        let mut transaction = self.pool.begin().await?;
        insert_session(
            &mut transaction,
            family_id,
            "access",
            &hash_token(&access),
            now,
            access_expires,
            absolute,
        )
        .await?;
        insert_session(
            &mut transaction,
            family_id,
            "refresh",
            &hash_token(&refresh),
            now,
            refresh_expires,
            absolute,
        )
        .await?;
        transaction.commit().await?;

        Ok(AuthSession {
            access_token: access,
            refresh_token: refresh,
            access_expires_at: access_expires,
            refresh_expires_at: refresh_expires,
        })
    }
}

/// Inserts a session row inside an existing transaction.
#[allow(clippy::too_many_arguments)]
async fn insert_session(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    family_id: &str,
    kind: &str,
    token_hash: &str,
    now: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    absolute: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO auth_sessions
            (id, token_hash, kind, family_id, created_at, expires_at, absolute_expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(token_hash)
    .bind(kind)
    .bind(family_id)
    .bind(now)
    .bind(expires_at)
    .bind(absolute)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn live(row: &SessionRow, now: DateTime<Utc>) -> bool {
    row.revoked_at.is_none() && row.expires_at > now
}

fn refresh_expiry(now: DateTime<Utc>, absolute: DateTime<Utc>) -> DateTime<Utc> {
    let sliding = now + Duration::days(REFRESH_TTL_DAYS);
    if sliding < absolute {
        sliding
    } else {
        absolute
    }
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| Error::Internal(format!("cannot hash the password: {error}")))?
        .to_string())
}

/// Verifies a password against a stored Argon2 hash.
pub fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// 256 bits of randomness, hex-encoded.
fn generate_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    format!("{digest:x}")
}

/// Human-readable one-time setup code, e.g. `8f3a-2b91-c4d7-e5f6`.
pub fn generate_setup_code() -> String {
    let raw = generate_token();
    raw.as_bytes()
        .chunks(4)
        .take(4)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("-")
}

/// Constant-time comparison of the setup code against the expected value.
pub fn setup_code_matches(provided: &str, expected: &str) -> bool {
    let provided = Sha256::digest(provided.trim().to_lowercase().as_bytes());
    let expected = Sha256::digest(expected.trim().to_lowercase().as_bytes());
    provided
        .iter()
        .zip(expected.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Passwords must be at least this long, so a LAN box is not trivially guessed.
pub const MIN_PASSWORD_LEN: usize = 8;

pub fn validate_password(password: &str) -> Result<()> {
    if password.len() < MIN_PASSWORD_LEN || password.len() > 1024 {
        return Err(Error::BadRequest(format!(
            "password must be between {MIN_PASSWORD_LEN} and 1024 characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    async fn repository() -> AuthRepository {
        let db = Db::connect_in_memory().await.expect("db");
        AuthRepository::new(db.pool.clone())
    }

    #[tokio::test]
    async fn login_requires_a_password_and_verifies_it() {
        let auth = repository().await;
        assert!(!auth.password_set().await.expect("state"));
        assert!(auth.login("secret123").await.is_err());

        auth.set_password("secret123").await.expect("set");
        assert!(auth.password_set().await.expect("state"));
        assert!(auth.login("wrong-one").await.is_err());

        let session = auth.login("secret123").await.expect("login");
        assert!(
            auth.authenticate(&session.access_token)
                .await
                .expect("auth")
        );
        assert!(
            !auth
                .authenticate(&session.refresh_token)
                .await
                .expect("auth")
        );
    }

    #[tokio::test]
    async fn refresh_rotates_and_reuse_revokes_the_family() {
        let auth = repository().await;
        auth.set_password("secret123").await.expect("set");
        let session = auth.login("secret123").await.expect("login");

        let Rotation::Rotated(rotated) =
            auth.refresh(&session.refresh_token).await.expect("refresh")
        else {
            panic!("first refresh must rotate");
        };
        assert!(
            auth.authenticate(&rotated.access_token)
                .await
                .expect("auth")
        );

        // The old refresh token is now a theft signal: the new one dies too.
        assert!(matches!(
            auth.refresh(&session.refresh_token).await.expect("reuse"),
            Rotation::Reused
        ));
        assert!(
            !auth
                .authenticate(&rotated.access_token)
                .await
                .expect("auth")
        );
        assert!(matches!(
            auth.refresh(&rotated.refresh_token).await.expect("refresh"),
            Rotation::Missing
        ));
    }

    #[tokio::test]
    async fn changing_the_password_revokes_every_session() {
        let auth = repository().await;
        auth.set_password("secret123").await.expect("set");
        let session = auth.login("secret123").await.expect("login");

        auth.set_password("another-secret").await.expect("replace");
        assert!(
            !auth
                .authenticate(&session.access_token)
                .await
                .expect("auth")
        );
        assert!(auth.login("secret123").await.is_err());
        assert!(auth.login("another-secret").await.is_ok());
    }

    #[test]
    fn setup_codes_and_passwords_are_validated() {
        let code = generate_setup_code();
        assert_eq!(code.len(), 19, "4 groups of 4 hex chars: {code}");
        assert!(setup_code_matches(&code.to_uppercase(), &code));
        assert!(!setup_code_matches("abcd-ef01-2345-6789", &code));

        assert!(validate_password("short").is_err());
        assert!(validate_password("long-enough").is_ok());
    }
}
