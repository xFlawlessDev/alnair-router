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
        assert_eq!(version, 24);

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
        assert_eq!(max_version, 24);
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
