//! Database backup and restore for the admin dashboard.
//!
//! Backup writes a consistent snapshot with SQLite's `VACUUM INTO` while the
//! router keeps serving. The `settings` row is dropped from the copy: it can
//! carry the admin token, and restore ignores it anyway. Restore validates the
//! uploaded file (SQLite integrity, matching migration set, decryptable
//! credentials) and replaces every data table inside one transaction, leaving
//! `settings` alone so the local runtime configuration survives an import.

use std::path::Path;

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{Connection, Executor, SqliteConnection, SqlitePool};

use crate::crypto::CredentialCipher;
use crate::error::{Error, Result};

/// Data tables copied on restore, parents before children.
const DATA_TABLES: &[&str] = &[
    "connections",
    "aliases",
    "combos",
    "combo_entries",
    "key_plans",
    "api_keys",
    "usage_records",
    "model_prices",
    "pricing_sync_runs",
];

/// Row count for one restored table.
#[derive(Debug, Serialize)]
pub struct TableCount {
    pub table: &'static str,
    pub rows: i64,
}

/// What a restore replaced, reported back to the dashboard.
#[derive(Debug, Serialize)]
pub struct RestoreSummary {
    pub tables: Vec<TableCount>,
    pub total_rows: i64,
}

/// Writes a consistent snapshot of `pool` to `destination`.
pub async fn snapshot(pool: &SqlitePool, destination: &Path) -> Result<()> {
    let target = destination.to_string_lossy().to_string();
    sqlx::query("VACUUM INTO ?")
        .bind(&target)
        .execute(pool)
        .await
        .map_err(|error| Error::Internal(format!("cannot create the backup: {error}")))?;

    // SQLite treats VACUUM as a no-op for in-memory databases, so it can report
    // success without writing anything.
    if !destination.exists() {
        return Err(Error::Internal(
            "the database did not produce a backup file; in-memory databases cannot be backed up"
                .to_string(),
        ));
    }

    let mut connection = open_file(destination).await?;
    let deleted = sqlx::query("DELETE FROM settings")
        .execute(&mut connection)
        .await;
    connection.close().await.ok();
    deleted.map_err(|error| Error::Internal(format!("cannot sanitize the backup: {error}")))?;

    Ok(())
}

/// Replaces every data table with the SQLite file at `source`.
pub async fn restore(
    pool: &SqlitePool,
    source: &Path,
    cipher: &CredentialCipher,
) -> Result<RestoreSummary> {
    let mut connection = pool.acquire().await?;
    sqlx::query("ATTACH DATABASE ? AS imported")
        .bind(source.to_string_lossy().to_string())
        .execute(&mut *connection)
        .await
        .map_err(|error| Error::BadRequest(format!("cannot read the backup: {error}")))?;

    let result = copy_into_main(&mut connection, cipher).await;

    if let Err(error) = sqlx::query("DETACH DATABASE imported")
        .execute(&mut *connection)
        .await
    {
        tracing::warn!(%error, "could not detach the imported backup");
    }

    result
}

async fn copy_into_main(
    connection: &mut SqliteConnection,
    cipher: &CredentialCipher,
) -> Result<RestoreSummary> {
    verify_schema(connection).await?;
    verify_credentials(connection, cipher).await?;

    // Deferred checks let the deletes and inserts run in any order while still
    // validating the final state at commit; the pragma resets with the commit.
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *connection)
        .await?;

    let mut transaction = connection.begin().await?;

    for &table in DATA_TABLES {
        sqlx::query(&format!("DELETE FROM main.{table}"))
            .execute(&mut *transaction)
            .await
            .map_err(|error| restore_failed(table, error))?;
    }

    let mut tables = Vec::with_capacity(DATA_TABLES.len());
    let mut total_rows = 0;
    for &table in DATA_TABLES {
        // The current schema is authoritative for the column list; the backup
        // must match it because the migration sets were checked above.
        let columns = column_list(&mut *transaction, table).await?;
        sqlx::query(&format!(
            "INSERT INTO main.{table} ({columns}) SELECT {columns} FROM imported.{table}"
        ))
        .execute(&mut *transaction)
        .await
        .map_err(|error| restore_failed(table, error))?;

        let rows: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM main.{table}"))
            .fetch_one(&mut *transaction)
            .await?;
        total_rows += rows;
        tables.push(TableCount { table, rows });
    }

    transaction.commit().await.map_err(|error| {
        Error::BadRequest(format!("the backup is not internally consistent: {error}"))
    })?;

    Ok(RestoreSummary { tables, total_rows })
}

/// Validates that the attached file is a healthy alnair-router database with
/// the same schema as the running one.
async fn verify_schema(connection: &mut SqliteConnection) -> Result<()> {
    let checks: Vec<(String,)> = sqlx::query_as("PRAGMA imported.quick_check")
        .fetch_all(&mut *connection)
        .await
        .map_err(|error| {
            Error::BadRequest(format!(
                "the upload is not a readable SQLite database: {error}"
            ))
        })?;
    if checks.len() != 1 || checks[0].0 != "ok" {
        return Err(Error::BadRequest(
            "the backup failed SQLite's integrity check".to_string(),
        ));
    }

    let current: Vec<i64> = sqlx::query_scalar(
        "SELECT version FROM main._sqlx_migrations WHERE success = 1 ORDER BY version",
    )
    .fetch_all(&mut *connection)
    .await?;
    let imported: Vec<i64> = sqlx::query_scalar(
        "SELECT version FROM imported._sqlx_migrations WHERE success = 1 ORDER BY version",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(|_| {
        Error::BadRequest("the backup does not look like an alnair-router database".to_string())
    })?;
    if current != imported {
        return Err(Error::BadRequest(
            "the backup was written by a different router version; download a fresh backup \
             from the matching version before restoring"
                .to_string(),
        ));
    }

    for &table in DATA_TABLES {
        let exists: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM imported.sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_optional(&mut *connection)
        .await?;
        if exists.is_none() {
            return Err(Error::BadRequest(format!(
                "the backup is missing the '{table}' table"
            )));
        }
    }

    // `quick_check` does not validate foreign keys.
    let violations = sqlx::query("PRAGMA imported.foreign_key_check")
        .fetch_all(&mut *connection)
        .await?;
    if !violations.is_empty() {
        return Err(Error::BadRequest(
            "the backup has broken references between its rows".to_string(),
        ));
    }

    Ok(())
}

/// Refuses credentials that the current `secrets.key` cannot decrypt.
async fn verify_credentials(
    connection: &mut SqliteConnection,
    cipher: &CredentialCipher,
) -> Result<()> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT name, api_key FROM imported.connections WHERE api_key IS NOT NULL")
            .fetch_all(&mut *connection)
            .await?;

    for (name, stored) in rows {
        cipher.decrypt(&stored).map_err(|_| {
            Error::BadRequest(format!(
                "connection '{name}' is encrypted with a different secrets.key; \
                 restore needs the key that wrote the backup"
            ))
        })?;
    }

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, secret_enc FROM imported.api_keys WHERE secret_enc IS NOT NULL",
    )
    .fetch_all(&mut *connection)
    .await?;

    for (name, stored) in rows {
        cipher.decrypt(&stored).map_err(|_| {
            Error::BadRequest(format!(
                "api key '{name}' is encrypted with a different secrets.key; \
                 restore needs the key that wrote the backup"
            ))
        })?;
    }

    Ok(())
}

async fn column_list<'e, E>(executor: E, table: &str) -> Result<String>
where
    E: Executor<'e, Database = sqlx::Sqlite>,
{
    let columns: Vec<(String,)> = sqlx::query_as("SELECT name FROM pragma_table_info(?)")
        .bind(table)
        .fetch_all(executor)
        .await?;
    if columns.is_empty() {
        return Err(Error::Internal(format!(
            "no column information for '{table}'"
        )));
    }

    Ok(columns
        .into_iter()
        .map(|(name,)| name)
        .collect::<Vec<_>>()
        .join(", "))
}

fn restore_failed(table: &str, error: sqlx::Error) -> Error {
    Error::Internal(format!("restore failed on '{table}': {error}"))
}

/// Opens the snapshot file for the post-copy cleanup without touching WAL.
async fn open_file(path: &Path) -> Result<SqliteConnection> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .journal_mode(SqliteJournalMode::Delete);
    SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| {
            Error::Internal(format!(
                "cannot open the backup copy {}: {error}",
                path.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RouterConfig, SecretsConfig};
    use crate::db::Db;
    use crate::settings::SettingsOverrides;

    const SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn cipher(secret: &str) -> CredentialCipher {
        CredentialCipher::from_config(&SecretsConfig {
            key: Some(secret.to_string()),
        })
        .expect("cipher")
    }

    /// `VACUUM INTO` is a silent no-op on in-memory databases, so snapshots
    /// need a real file.
    async fn file_db(directory: &tempfile::TempDir) -> Db {
        let mut config = RouterConfig::default();
        config.secrets.key = Some(SECRET.to_string());
        config.storage.url = format!(
            "sqlite://{}?mode=rwc",
            directory.path().join("router.sqlite").display()
        );

        let db = Db::connect(&config).await.expect("db");
        db.migrate().await.expect("migrate");
        db
    }

    async fn seed_connection(db: &Db, cipher: &CredentialCipher, name: &str) {
        sqlx::query(
            "INSERT INTO connections (id, name, provider_type, base_url, api_key, custom_headers,
             enabled, created_at, updated_at)
             VALUES (?, ?, 'openai-compatible', 'https://example.invalid/v1', ?, '{}', 1,
             datetime('now'), datetime('now'))",
        )
        .bind(format!("id-{name}"))
        .bind(name)
        .bind(cipher.encrypt("sk-upstream").expect("encrypt"))
        .execute(&db.pool)
        .await
        .expect("insert connection");
    }

    #[tokio::test]
    async fn snapshot_deletes_settings_and_restore_keeps_them() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db = file_db(&directory).await;
        let source = cipher(SECRET);
        seed_connection(&db, &source, "main").await;

        // Settings are local state: stored before the snapshot, untouched by restore.
        let settings = SettingsOverrides {
            require_api_key: Some(true),
            ..SettingsOverrides::default()
        };
        crate::settings::SettingsRepository::new(db.pool.clone())
            .save(&settings)
            .await
            .expect("save settings");

        let path = directory.path().join("backup.sqlite");
        snapshot(&db.pool, &path).await.expect("snapshot");
        assert!(path.exists(), "snapshot did not create {path:?}");

        // The exported file carries no settings row.
        let mut exported = open_file(&path).await.expect("open export");
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settings")
            .fetch_one(&mut exported)
            .await
            .expect("count");
        assert_eq!(rows, 0);
        drop(exported);

        sqlx::query("DELETE FROM connections")
            .execute(&db.pool)
            .await
            .expect("wipe");

        let summary = restore(&db.pool, &path, &source).await.expect("restore");
        assert_eq!(summary.total_rows, 1);

        let connections: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM connections")
            .fetch_one(&db.pool)
            .await
            .expect("count");
        assert_eq!(connections, 1);

        let stored = crate::settings::SettingsRepository::new(db.pool.clone())
            .get()
            .await
            .expect("settings");
        assert_eq!(stored.require_api_key, Some(true));
    }

    #[tokio::test]
    async fn restore_rejects_credentials_from_a_different_key() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db = file_db(&directory).await;
        seed_connection(&db, &cipher(SECRET), "main").await;

        let path = directory.path().join("backup.sqlite");
        snapshot(&db.pool, &path).await.expect("snapshot");

        let other = cipher(&"cd".repeat(32));
        let error = restore(&db.pool, &path, &other)
            .await
            .expect_err("must refuse");
        assert!(matches!(error, Error::BadRequest(_)));
        assert!(error.to_string().contains("secrets.key"));
    }

    /// Inserts a key row directly, so the encrypted secret can be written under
    /// a nominated cipher.
    async fn seed_api_key(db: &Db, cipher: &CredentialCipher, name: &str) {
        sqlx::query(
            "INSERT INTO api_keys (id, name, key_hash, secret_enc, prefix, enabled, budget_mode,
             created_at)
             VALUES (?, ?, 'hash', ?, 'sk-router-', 1, 'block', datetime('now'))",
        )
        .bind(format!("id-{name}"))
        .bind(name)
        .bind(cipher.encrypt("sk-router-secret").expect("encrypt"))
        .execute(&db.pool)
        .await
        .expect("insert api key");
    }

    #[tokio::test]
    async fn restore_rejects_key_secrets_from_a_different_key() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db = file_db(&directory).await;
        seed_api_key(&db, &cipher(SECRET), "main").await;

        let path = directory.path().join("backup.sqlite");
        snapshot(&db.pool, &path).await.expect("snapshot");

        let other = cipher(&"cd".repeat(32));
        let error = restore(&db.pool, &path, &other)
            .await
            .expect_err("must refuse");
        assert!(matches!(error, Error::BadRequest(_)));
        assert!(error.to_string().contains("secrets.key"));
        assert!(
            error.to_string().contains("api key 'main'"),
            "the message should name the offending row: {error}"
        );
    }

    #[tokio::test]
    async fn restore_accepts_key_secrets_written_under_the_same_key() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db = file_db(&directory).await;
        let source = cipher(SECRET);
        seed_api_key(&db, &source, "main").await;

        let path = directory.path().join("backup.sqlite");
        snapshot(&db.pool, &path).await.expect("snapshot");

        let summary = restore(&db.pool, &path, &source).await.expect("restore");
        assert_eq!(summary.total_rows, 1);

        let stored: String =
            sqlx::query_scalar("SELECT secret_enc FROM api_keys WHERE id = 'id-main'")
                .fetch_one(&db.pool)
                .await
                .expect("secret_enc");
        assert_eq!(
            source.decrypt(&stored).expect("decrypt"),
            "sk-router-secret"
        );
    }

    #[tokio::test]
    async fn restore_rejects_files_that_are_not_sqlite() {
        let db = Db::connect_in_memory().await.expect("db");
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("garbage.sqlite");
        std::fs::write(&path, b"definitely not a database").expect("write");

        let error = restore(&db.pool, &path, &cipher(SECRET))
            .await
            .expect_err("must refuse");
        assert!(matches!(error, Error::BadRequest(_)));
    }
}
