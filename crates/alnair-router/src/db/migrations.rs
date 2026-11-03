use std::borrow::Cow;

use sha2::{Digest, Sha384};
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

use crate::error::Result;

/// Normalizes migration SQL line endings and repairs only matching legacy
/// checksums created by a Windows CRLF checkout.
///
/// SQLx checksums the exact embedded file bytes. A Windows build may therefore
/// record CRLF checksums while a later Linux build embeds the same SQL with LF.
/// New builds always run a canonical LF migrator. Existing metadata is updated
/// only when its checksum exactly equals the CRLF form of that same migration
/// and the migration was successfully applied. Application tables and rows
/// are never read or modified by this reconciliation.
pub(super) async fn canonical_migrator(pool: &SqlitePool) -> Result<Migrator> {
    let source = sqlx::migrate!("./migrations");
    let has_migration_table: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    let mut migrations = Vec::with_capacity(source.iter().len());

    for source_migration in source.iter() {
        let mut migration = source_migration.clone();
        let canonical_sql = migration.sql.replace("\r\n", "\n");
        let canonical_checksum = Sha384::digest(canonical_sql.as_bytes()).to_vec();
        let crlf_sql = canonical_sql.replace('\n', "\r\n");
        let crlf_checksum = Sha384::digest(crlf_sql.as_bytes()).to_vec();

        let applied: Option<(Vec<u8>, i64)> = if has_migration_table {
            sqlx::query_as("SELECT checksum, success FROM _sqlx_migrations WHERE version = ?")
                .bind(migration.version)
                .fetch_optional(pool)
                .await?
        } else {
            None
        };

        if let Some((stored_checksum, 1)) = applied
            && stored_checksum == crlf_checksum
            && crlf_checksum != canonical_checksum
        {
            sqlx::query(
                "UPDATE _sqlx_migrations SET checksum = ? WHERE version = ? AND success = 1",
            )
            .bind(&canonical_checksum)
            .bind(migration.version)
            .execute(pool)
            .await?;
            tracing::info!(
                migration = migration.version,
                "normalized legacy Windows CRLF migration checksum; application data was untouched"
            );
        }

        migration.sql = Cow::Owned(canonical_sql);
        migration.checksum = Cow::Owned(canonical_checksum);
        migrations.push(migration);
    }

    Ok(Migrator {
        migrations: Cow::Owned(migrations),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    })
}

#[cfg(test)]
use sqlx::sqlite::SqlitePoolOptions;

/// Creates a one-connection pool for an isolated migration test.
#[cfg(test)]
pub(super) async fn test_pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider-type migration rebuilds `connections`, which every child table
    /// references. With foreign keys on, dropping the old table would cascade
    /// into those children, so the rebuild copies them aside and restores them.
    /// This drives the real migrations and then asserts the children survived.
    #[tokio::test]
    async fn provider_type_rebuild_keeps_children() {
        use crate::config::SecretsConfig;
        use crate::crypto::CredentialCipher;
        use std::sync::Arc;

        const SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

        let db = crate::db::Db::connect_in_memory().await.expect("db");
        let cipher = Arc::new(
            CredentialCipher::from_config(&SecretsConfig {
                key: Some(SECRET.to_string()),
            })
            .expect("cipher"),
        );

        // A connection with every child row kind attached.
        let connection = crate::db::repos::connections::ConnectionRepository::new(
            db.pool.clone(),
            cipher.clone(),
        )
        .create(crate::db::repos::connections::CreateConnection {
            name: "codex-main".to_string(),
            provider_type: "openai-codex".to_string(),
            base_url: "https://chatgpt.com/backend-api/codex".to_string(),
            api_key: None,
            custom_headers: Default::default(),
            enabled: true,
            connect_timeout_ms: None,
            idle_timeout_ms: None,
            pricing_model: None,
            cache_retention: None,
            auth_style: Some("bearer".to_string()),
            web_fetch_mode: None,
            web_fetch_path: None,

            search_path: None,

            search_tier: None,
            provider_id: Some("openai-codex".to_string()),
        })
        .await
        .expect("a codex connection is accepted");

        // Columns added after an earlier rebuild must survive this one.
        assert_eq!(connection.auth_style, "bearer");
        assert_eq!(connection.cache_retention, "none");
        assert_eq!(connection.web_fetch_mode, "local");

        sqlx::query(
            "INSERT INTO aliases (id, prefix, connection_id, model_override, enabled, sort_order, created_at, updated_at)
             VALUES ('alias-1', 'codex', ?, NULL, 1, 0, 'now', 'now')",
        )
        .bind(&connection.id)
        .execute(&db.pool)
        .await
        .expect("alias");

        sqlx::query(
            "INSERT INTO oauth_accounts (id, connection_id, label, provider_key, credential, enabled, created_at, updated_at)
             VALUES ('account-1', ?, 'work', 'openai-codex', '{}', 1, 'now', 'now')",
        )
        .bind(&connection.id)
        .execute(&db.pool)
        .await
        .expect("oauth account");

        let aliases: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM aliases")
            .fetch_one(&db.pool)
            .await
            .expect("aliases survived");
        assert_eq!(aliases, 1, "the rebuild dropped an alias");

        let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM oauth_accounts")
            .fetch_one(&db.pool)
            .await
            .expect("oauth accounts survived");
        assert_eq!(accounts, 1, "the rebuild dropped an OAuth account");

        // An unsupported family is still refused by the rebuilt CHECK.
        let error = sqlx::query("UPDATE connections SET provider_type = 'not-a-provider'")
            .execute(&db.pool)
            .await
            .expect_err("the CHECK must still reject an unknown family");
        assert!(error.to_string().to_lowercase().contains("constraint"));
    }

    #[tokio::test]
    async fn windows_crlf_checksum_is_reconciled_without_changing_data() {
        let pool = test_pool().await;
        let migrator = canonical_migrator(&pool).await.expect("canonical migrator");
        migrator.run(&pool).await.expect("initial schema");
        sqlx::query("INSERT INTO settings (id, data, updated_at) VALUES (1, '{}', 'now')")
            .execute(&pool)
            .await
            .expect("seed application data");

        // Simulate a Windows checkout for every migration the old binary applied.
        let source = sqlx::migrate!("./migrations");
        for migration in source.iter() {
            let crlf_sql = migration.sql.replace("\r\n", "\n").replace('\n', "\r\n");
            let crlf_checksum = Sha384::digest(crlf_sql.as_bytes()).to_vec();
            sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
                .bind(crlf_checksum)
                .bind(migration.version)
                .execute(&pool)
                .await
                .expect("simulate a Windows build");
        }

        let windows_v1_checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
                .fetch_one(&pool)
                .await
                .expect("Windows version-1 checksum");
        assert_eq!(
            hex::encode_upper(&windows_v1_checksum),
            "B824B231DDDA7792201F8D23506C9FCAAE56E2E4EC07A1AE8E09C50CC0CDA22319E90765222F915096EA5BCC5C7711FA"
        );

        let migrator = canonical_migrator(&pool)
            .await
            .expect("reconcile checksums");
        migrator.run(&pool).await.expect("continue migrations");

        let setting: String = sqlx::query_scalar("SELECT data FROM settings WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("application data survived");
        assert_eq!(setting, "{}");

        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .expect("latest migration");
        assert_eq!(version, 28);

        let stored: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
                .fetch_one(&pool)
                .await
                .expect("canonical version-1 checksum");
        let source = sqlx::migrate!("./migrations");
        let expected_v1_sql = source
            .iter()
            .find(|migration| migration.version == 1)
            .expect("version-1 migration")
            .sql
            .replace("\r\n", "\n");
        let expected_v1_checksum = Sha384::digest(expected_v1_sql.as_bytes()).to_vec();
        assert_eq!(stored, expected_v1_checksum);

        for migration in source.iter() {
            let expected_sql = migration.sql.replace("\r\n", "\n");
            let expected_checksum = Sha384::digest(expected_sql.as_bytes()).to_vec();
            let checksum: Vec<u8> =
                sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
                    .bind(migration.version)
                    .fetch_one(&pool)
                    .await
                    .expect("canonical migration checksum");
            assert_eq!(checksum, expected_checksum);
        }
    }

    #[tokio::test]
    async fn full_database_migrate_repairs_the_reported_windows_checksum() {
        let pool = test_pool().await;
        let db = crate::db::Db { pool };
        db.migrate().await.expect("initial database migrations");

        let source = sqlx::migrate!("./migrations");
        for migration in source.iter() {
            let crlf_sql = migration.sql.replace("\r\n", "\n").replace('\n', "\r\n");
            let checksum = Sha384::digest(crlf_sql.as_bytes()).to_vec();
            sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
                .bind(checksum)
                .bind(migration.version)
                .execute(&db.pool)
                .await
                .expect("simulate Windows migration checksum");
        }

        let before: String =
            sqlx::query_scalar("SELECT hex(checksum) FROM _sqlx_migrations WHERE version = 1")
                .fetch_one(&db.pool)
                .await
                .expect("affected database checksum");
        assert_eq!(
            before,
            "B824B231DDDA7792201F8D23506C9FCAAE56E2E4EC07A1AE8E09C50CC0CDA22319E90765222F915096EA5BCC5C7711FA"
        );

        db.migrate()
            .await
            .expect("upgrade without deleting database");
        let max_version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&db.pool)
            .await
            .expect("latest migration version");
        assert_eq!(max_version, 28);
    }

    #[tokio::test]
    async fn does_not_rewrite_a_checksum_that_is_not_the_crlf_variant() {
        let pool = test_pool().await;
        let migrator = canonical_migrator(&pool).await.expect("canonical migrator");
        migrator.run(&pool).await.expect("initial schema");

        let bogus = vec![0xA5; 48];
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
            .bind(&bogus)
            .execute(&pool)
            .await
            .expect("simulate unrelated checksum mismatch");

        let _ = canonical_migrator(&pool).await.expect("inspect checksums");
        let stored: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
                .fetch_one(&pool)
                .await
                .expect("unchanged checksum");
        assert_eq!(stored, bogus);
    }
}
