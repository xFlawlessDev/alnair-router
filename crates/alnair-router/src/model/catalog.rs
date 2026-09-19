//! Loads a [`Catalog`] snapshot from the database.

use sqlx::SqlitePool;

use super::resolver::Catalog;
use crate::crypto::CredentialCipher;
use crate::error::Result;

impl Catalog {
    /// Reads the current routing configuration into a snapshot.
    ///
    /// Connection API keys are decrypted so resolvers hand plaintext
    /// credentials to the provider layer.
    pub async fn load(pool: &SqlitePool, cipher: &CredentialCipher) -> Result<Self> {
        let mut connections = sqlx::query_as::<_, crate::db::repos::connections::Connection>(
            "SELECT * FROM connections",
        )
        .fetch_all(pool)
        .await?;

        for connection in &mut connections {
            connection.decrypt_api_key(cipher)?;
        }

        // Enabled extra keys join the connection's rotation list.
        let accounts = sqlx::query_as::<_, (String, String)>(
            "SELECT connection_id, api_key FROM connection_accounts
             WHERE enabled = 1 ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        let mut extra: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for (connection_id, stored) in accounts {
            extra
                .entry(connection_id)
                .or_default()
                .push(cipher.decrypt(&stored)?);
        }
        for connection in &mut connections {
            connection.extra_keys = extra.remove(&connection.id).unwrap_or_default();
        }

        let aliases =
            sqlx::query_as::<_, crate::db::repos::aliases::Alias>("SELECT * FROM aliases")
                .fetch_all(pool)
                .await?;

        let combos = sqlx::query_as::<_, crate::db::repos::combos::Combo>("SELECT * FROM combos")
            .fetch_all(pool)
            .await?;

        let combo_entries = sqlx::query_as::<_, crate::db::repos::combos::ComboEntry>(
            "SELECT * FROM combo_entries",
        )
        .fetch_all(pool)
        .await?;

        Ok(Catalog {
            connections,
            aliases,
            combos,
            combo_entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::config::SecretsConfig;
    use crate::db::Db;
    use crate::db::repos::connection_accounts::{
        ConnectionAccountRepository, CreateConnectionAccount,
    };
    use crate::db::repos::connections::{ConnectionRepository, CreateConnection};

    const SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[tokio::test]
    async fn catalog_attaches_enabled_extra_keys_to_their_connection() {
        let db = Db::connect_in_memory().await.expect("db");
        let cipher = Arc::new(
            CredentialCipher::from_config(&SecretsConfig {
                key: Some(SECRET.to_string()),
            })
            .expect("cipher"),
        );

        let connection = ConnectionRepository::new(db.pool.clone(), cipher.clone())
            .create(CreateConnection {
                name: "openai-main".to_string(),
                provider_type: "openai-compatible".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                api_key: Some("sk-primary".to_string()),
                custom_headers: Default::default(),
                enabled: true,
                connect_timeout_ms: None,
                idle_timeout_ms: None,
                pricing_model: None,
                cache_retention: None,
                auth_style: None,
                provider_id: None,
            })
            .await
            .expect("connection");

        let accounts = ConnectionAccountRepository::new(db.pool.clone(), cipher.clone());
        accounts
            .create(
                &connection.id,
                CreateConnectionAccount {
                    label: "enabled".to_string(),
                    api_key: "sk-extra".to_string(),
                    enabled: true,
                },
            )
            .await
            .expect("enabled account");
        accounts
            .create(
                &connection.id,
                CreateConnectionAccount {
                    label: "disabled".to_string(),
                    api_key: "sk-off".to_string(),
                    enabled: false,
                },
            )
            .await
            .expect("disabled account");

        let catalog = Catalog::load(&db.pool, &cipher).await.expect("catalog");
        assert_eq!(
            catalog.connections[0].extra_keys,
            vec!["sk-extra".to_string()],
            "disabled accounts stay out of the rotation"
        );

        let resolver = catalog.resolver(Some("openai-main".to_string()), 5);
        let targets = resolver.resolve("gpt-4o").expect("targets");
        assert_eq!(
            targets[0].api_keys,
            vec!["sk-primary".to_string(), "sk-extra".to_string()]
        );
    }
}
