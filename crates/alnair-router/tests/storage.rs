//! Storage layer tests against an in-memory SQLite database.

use alnair_router::crypto::CredentialCipher;
use alnair_router::db::Db;
use alnair_router::db::repos::aliases::{AliasRepository, CreateAlias};
use alnair_router::db::repos::api_keys::{ApiKeyRepository, CreateApiKey, UpdateApiKey};
use alnair_router::db::repos::combos::{ComboRepository, CreateCombo};
use alnair_router::db::repos::connections::{
    ConnectionRepository, CreateConnection, UpdateConnection,
};
use alnair_router::db::repos::usage::{NewUsageRecord, Sort, UsageFilter, UsageRepository};
use alnair_router::limits::{BudgetMode, BudgetWindow};
use alnair_router::pricing::{
    FetchedPrice, Price, PriceInput, PricingCache, PricingRepository, PricingSyncStatus,
};
use alnair_router::token_saver::SavingsTotals;
use alnair_router::{Error, Result};

async fn db() -> Db {
    Db::connect_in_memory().await.expect("in-memory db")
}

fn test_cipher() -> std::sync::Arc<CredentialCipher> {
    std::sync::Arc::new(CredentialCipher::ephemeral())
}

fn connection_repo(db: &Db) -> ConnectionRepository {
    ConnectionRepository::new(db.pool.clone(), test_cipher())
}

fn api_key(name: &str) -> CreateApiKey {
    CreateApiKey {
        name: name.to_string(),
        enabled: true,
        rate_limit_per_minute: None,
        daily_budget_usd: None,
        weekly_budget_usd: None,
        monthly_budget_usd: None,
        lifetime_budget_usd: None,
        daily_token_limit: None,
        weekly_token_limit: None,
        monthly_token_limit: None,
        lifetime_token_limit: None,
        budget_mode: None,
        plan_id: None,
        allowed_models: None,
        expires_at: None,
    }
}

fn connection(name: &str, provider_type: &str) -> CreateConnection {
    CreateConnection {
        name: name.to_string(),
        provider_type: provider_type.to_string(),
        base_url: "https://api.example.com/v1".to_string(),
        api_key: Some("sk-test".to_string()),
        custom_headers: Default::default(),
        enabled: true,
        connect_timeout_ms: None,
        idle_timeout_ms: None,
        pricing_model: None,
        provider_id: None,
    }
}

#[tokio::test]
async fn migrations_create_all_tables() {
    let db = db().await;
    let tables: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .fetch_all(&db.pool)
            .await
            .expect("list tables");

    let names: Vec<String> = tables.into_iter().map(|(name,)| name).collect();
    for expected in [
        "aliases",
        "api_keys",
        "combo_entries",
        "combos",
        "connections",
        "usage_records",
    ] {
        assert!(
            names.iter().any(|t| t == expected),
            "missing table {expected}"
        );
    }
}

#[tokio::test]
async fn connection_create_and_read_back() {
    let db = db().await;
    let repo = connection_repo(&db);

    let created = repo
        .create(connection("openai-main", "openai-compatible"))
        .await
        .expect("create connection");

    assert_eq!(created.name, "openai-main");
    assert_eq!(created.provider_type, "openai-compatible");
    assert_eq!(created.api_key.as_deref(), Some("sk-test"));
    assert!(created.is_enabled());

    let fetched = repo.get(&created.id).await.expect("get").expect("exists");
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.base_url, "https://api.example.com/v1");
}

#[tokio::test]
async fn connection_accepts_codebuddy_intl_provider_type() {
    let db = db().await;
    let repo = connection_repo(&db);

    let created = repo
        .create(connection("codebuddy", "codebuddy-intl"))
        .await
        .expect("codebuddy-intl must be accepted");

    assert_eq!(created.provider_type, "codebuddy-intl");
}

#[tokio::test]
async fn connection_rejects_ollama_provider_type() {
    let db = db().await;
    let repo = connection_repo(&db);

    let error = repo
        .create(connection("local", "ollama"))
        .await
        .expect_err("ollama must be rejected");

    assert!(matches!(error, Error::UnsupportedProviderType(ref value) if value == "ollama"));
}

#[tokio::test]
async fn connection_credentials_are_encrypted_at_rest() {
    let db = db().await;
    let cipher = test_cipher();
    let repo = ConnectionRepository::new(db.pool.clone(), cipher.clone());

    let created = repo
        .create(connection("openai-main", "openai-compatible"))
        .await
        .expect("create connection");

    // The repository returns plaintext to callers...
    assert_eq!(created.api_key.as_deref(), Some("sk-test"));

    // ...while the column only holds ciphertext.
    let stored: String = sqlx::query_scalar("SELECT api_key FROM connections WHERE id = ?")
        .bind(&created.id)
        .fetch_one(&db.pool)
        .await
        .expect("raw api_key");

    assert_ne!(stored, "sk-test");
    assert!(CredentialCipher::is_encrypted(&stored));
    assert_eq!(cipher.decrypt(&stored).expect("decrypt"), "sk-test");
}

#[tokio::test]
async fn connection_update_keeps_the_existing_key_encrypted() {
    let db = db().await;
    let cipher = test_cipher();
    let repo = ConnectionRepository::new(db.pool.clone(), cipher.clone());
    let created = repo
        .create(connection("main", "openai-compatible"))
        .await
        .expect("create");

    let updated = repo
        .update(
            &created.id,
            alnair_router::db::repos::connections::UpdateConnection {
                name: Some("renamed".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("update");

    assert_eq!(updated.api_key.as_deref(), Some("sk-test"));

    let stored: String = sqlx::query_scalar("SELECT api_key FROM connections WHERE id = ?")
        .bind(&created.id)
        .fetch_one(&db.pool)
        .await
        .expect("raw api_key");

    assert!(CredentialCipher::is_encrypted(&stored));
    assert_eq!(cipher.decrypt(&stored).expect("decrypt"), "sk-test");
}

#[tokio::test]
async fn boot_migration_encrypts_legacy_plaintext_credentials() {
    let db = db().await;
    sqlx::query(
        "INSERT INTO connections
            (id, name, provider_type, base_url, api_key, custom_headers, enabled, created_at, updated_at)
         VALUES
            ('legacy', 'legacy', 'openai-compatible', 'https://api.example.com/v1', 'sk-legacy', '{}', 1,
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&db.pool)
    .await
    .expect("insert legacy row");

    let cipher = CredentialCipher::ephemeral();
    let migrated = db.migrate_credentials(&cipher).await.expect("migrate");
    assert_eq!(migrated, 1, "one plaintext row should be rewritten");

    let stored: String = sqlx::query_scalar("SELECT api_key FROM connections WHERE id = 'legacy'")
        .fetch_one(&db.pool)
        .await
        .expect("raw api_key");
    assert!(CredentialCipher::is_encrypted(&stored));
    assert_eq!(cipher.decrypt(&stored).expect("decrypt"), "sk-legacy");
}

#[tokio::test]
async fn boot_migration_refuses_a_wrong_key() {
    let db = db().await;
    let writer = test_cipher();
    let repo = ConnectionRepository::new(db.pool.clone(), writer);
    repo.create(connection("main", "openai-compatible"))
        .await
        .expect("create");

    let other = CredentialCipher::ephemeral();
    let error = db
        .migrate_credentials(&other)
        .await
        .expect_err("wrong key must fail");
    assert!(matches!(error, Error::Config(_)));
}

#[tokio::test]
async fn connection_update_rejects_unsupported_type() {
    let db = db().await;
    let repo = connection_repo(&db);
    let created = repo
        .create(connection("main", "openai-compatible"))
        .await
        .expect("create");

    let error = repo
        .update(
            &created.id,
            alnair_router::db::repos::connections::UpdateConnection {
                provider_type: Some("ollama".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect_err("ollama must be rejected on update");

    assert!(matches!(error, Error::UnsupportedProviderType(_)));
}

#[tokio::test]
async fn connection_base_url_rejects_a_preset_placeholder() {
    let db = db().await;
    let repo = connection_repo(&db);

    let error = repo
        .create(CreateConnection {
            base_url: "https://<your-resource>.openai.azure.com/openai/v1".to_string(),
            ..connection("azure", "openai-compatible")
        })
        .await
        .expect_err("a placeholder must not be stored");
    assert!(matches!(error, Error::BadRequest(_)));

    // The same rule applies when the URL is changed later.
    let created = repo
        .create(connection("main", "openai-compatible"))
        .await
        .expect("create");
    let error = repo
        .update(
            &created.id,
            alnair_router::db::repos::connections::UpdateConnection {
                base_url: Some(
                    "https://api.cloudflare.com/client/v4/accounts/<ACCOUNT_ID>/ai/v1".to_string(),
                ),
                ..Default::default()
            },
        )
        .await
        .expect_err("a placeholder must not be stored on update");
    assert!(matches!(error, Error::BadRequest(_)));

    // An untouched row keeps working.
    let unchanged = repo
        .update(
            &created.id,
            alnair_router::db::repos::connections::UpdateConnection {
                name: Some("renamed".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("update");
    assert_eq!(unchanged.base_url, "https://api.example.com/v1");
}

/// The new provider family is admitted by the CHECK constraint, and the table
/// rebuild that widened it keeps child rows alive. `DROP TABLE connections`
/// cascades into `aliases`/`connection_accounts` unless the migration turns
/// foreign keys off first, which is what this pins down.
#[tokio::test]
async fn provider_type_rebuild_keeps_children() {
    let db = db().await;

    // Fresh databases already carry the widened constraint, so rebuild a
    // legacy-shaped connections table to run the migration against real data.
    sqlx::raw_sql(
        "PRAGMA foreign_keys = OFF;
         DROP TABLE connections;
         CREATE TABLE connections (
             id                 TEXT PRIMARY KEY,
             name               TEXT NOT NULL UNIQUE,
             provider_type      TEXT NOT NULL CHECK (provider_type IN ('openai-compatible', 'anthropic-native')),
             base_url           TEXT NOT NULL,
             api_key            TEXT,
             custom_headers     TEXT NOT NULL DEFAULT '{}',
             enabled            INTEGER NOT NULL DEFAULT 1,
             created_at         TEXT NOT NULL,
             updated_at         TEXT NOT NULL,
             connect_timeout_ms INTEGER,
             idle_timeout_ms    INTEGER,
             pricing_model      TEXT,
             provider_id        TEXT
         );
         PRAGMA foreign_keys = ON;",
    )
    .execute(&db.pool)
    .await
    .expect("legacy schema");

    let repo = connection_repo(&db);
    let seeded = repo
        .create(connection("legacy", "openai-compatible"))
        .await
        .expect("seed connection");

    sqlx::query(
        "INSERT INTO aliases (id, prefix, connection_id, model_override, enabled, sort_order, created_at, updated_at)
         VALUES ('a1', 'leg', ?, NULL, 1, 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&seeded.id)
    .execute(&db.pool)
    .await
    .expect("seed alias");

    sqlx::raw_sql(include_str!("../migrations/0017_command_code_provider.sql"))
        .execute(&db.pool)
        .await
        .expect("rebuild migration");

    let aliases: Vec<(String,)> = sqlx::query_as("SELECT prefix FROM aliases")
        .fetch_all(&db.pool)
        .await
        .expect("aliases survived");
    assert_eq!(aliases.len(), 1, "the rebuild must not cascade children");

    let rebuilt = repo
        .create(connection("go", "command-code"))
        .await
        .expect("command-code is admitted");
    assert_eq!(rebuilt.provider_type, "command-code");

    let error = repo
        .create(connection("nope", "ollama"))
        .await
        .expect_err("unknown types are still rejected");
    assert!(matches!(error, Error::UnsupportedProviderType(_)));
    assert!(
        sqlx::query("SELECT id FROM connections WHERE id = ?")
            .bind(&seeded.id)
            .fetch_optional(&db.pool)
            .await
            .expect("legacy row survived")
            .is_some(),
        "existing connections are carried over"
    );
}

#[tokio::test]
async fn connection_name_is_unique() {
    let db = db().await;
    let repo = connection_repo(&db);
    repo.create(connection("dup", "openai-compatible"))
        .await
        .expect("first");

    let error = repo
        .create(connection("dup", "anthropic-native"))
        .await
        .expect_err("duplicate name must fail");
    assert!(matches!(error, Error::Database(_)));
}

#[tokio::test]
async fn connection_custom_headers_roundtrip() {
    let db = db().await;
    let repo = connection_repo(&db);

    let mut input = connection("headers", "openai-compatible");
    input
        .custom_headers
        .insert("X-Org".to_string(), "acme".to_string());

    let created = repo.create(input).await.expect("create");
    let headers = created.headers();
    assert_eq!(headers.get("X-Org").map(String::as_str), Some("acme"));
}

#[tokio::test]
async fn alias_prefix_is_normalized_and_fetched() {
    let db = db().await;
    let connections = connection_repo(&db);
    let aliases = AliasRepository::new(db.pool.clone());

    let connection = connections
        .create(connection("openai-main", "openai-compatible"))
        .await
        .expect("connection");

    let alias = aliases
        .create(CreateAlias {
            prefix: "  GLM/  ".to_string(),
            connection_id: connection.id.clone(),
            model_override: None,
            enabled: true,
            sort_order: 0,
        })
        .await
        .expect("alias");

    assert_eq!(alias.prefix, "glm");

    let found = aliases
        .get_by_prefix("GLM")
        .await
        .expect("lookup")
        .expect("exists");
    assert_eq!(found.id, alias.id);
}

#[tokio::test]
async fn alias_rejects_prefix_with_slash() {
    let db = db().await;
    let connections = connection_repo(&db);
    let aliases = AliasRepository::new(db.pool.clone());
    let connection = connections
        .create(connection("main", "openai-compatible"))
        .await
        .expect("connection");

    let error = aliases
        .create(CreateAlias {
            prefix: "bad/prefix".to_string(),
            connection_id: connection.id,
            model_override: None,
            enabled: true,
            sort_order: 0,
        })
        .await
        .expect_err("slash must be rejected");

    assert!(matches!(error, Error::BadRequest(_)));
}

#[tokio::test]
async fn alias_model_override_blank_is_stored_as_none() {
    let db = db().await;
    let connections = connection_repo(&db);
    let aliases = AliasRepository::new(db.pool.clone());
    let connection = connections
        .create(connection("main", "openai-compatible"))
        .await
        .expect("connection");

    let alias = aliases
        .create(CreateAlias {
            prefix: "glm".to_string(),
            connection_id: connection.id,
            model_override: Some("   ".to_string()),
            enabled: true,
            sort_order: 0,
        })
        .await
        .expect("alias");

    assert_eq!(alias.model_override, None);
}

#[tokio::test]
async fn alias_delete_cascades_from_connection() {
    let db = db().await;
    let connections = connection_repo(&db);
    let aliases = AliasRepository::new(db.pool.clone());
    let connection = connections
        .create(connection("main", "openai-compatible"))
        .await
        .expect("connection");

    aliases
        .create(CreateAlias {
            prefix: "glm".to_string(),
            connection_id: connection.id.clone(),
            model_override: None,
            enabled: true,
            sort_order: 0,
        })
        .await
        .expect("alias");

    connections.delete(&connection.id).await.expect("delete");
    assert!(aliases.list().await.expect("list").is_empty());
}

#[tokio::test]
async fn combo_entries_preserve_declared_order() {
    let db = db().await;
    let combos = ComboRepository::new(db.pool.clone());

    let combo = combos
        .create(CreateCombo {
            name: "Free Forever".to_string(),
            description: Some("tiered".to_string()),
            enabled: true,
            entries: vec![
                "glm/glm-5.1".to_string(),
                "kr/claude-4.5".to_string(),
                "gh/gpt-5".to_string(),
            ],
        })
        .await
        .expect("combo");

    assert_eq!(combo.combo.name, "free forever");
    let refs: Vec<&str> = combo.entries.iter().map(|e| e.model_ref.as_str()).collect();
    assert_eq!(refs, vec!["glm/glm-5.1", "kr/claude-4.5", "gh/gpt-5"]);
    let positions: Vec<i64> = combo.entries.iter().map(|e| e.position).collect();
    assert_eq!(positions, vec![0, 1, 2]);
}

#[tokio::test]
async fn combo_update_replaces_entries_wholesale() {
    let db = db().await;
    let combos = ComboRepository::new(db.pool.clone());

    let combo = combos
        .create(CreateCombo {
            name: "chain".to_string(),
            description: None,
            enabled: true,
            entries: vec!["a/one".to_string(), "b/two".to_string()],
        })
        .await
        .expect("combo");

    let updated = combos
        .update(
            &combo.combo.id,
            alnair_router::db::repos::combos::UpdateCombo {
                name: None,
                description: None,
                enabled: None,
                entries: Some(vec!["c/three".to_string()]),
            },
        )
        .await
        .expect("update");

    assert_eq!(updated.entries.len(), 1);
    assert_eq!(updated.entries[0].model_ref, "c/three");
    assert_eq!(updated.entries[0].position, 0);
}

#[tokio::test]
async fn combo_delete_cascades_entries() {
    let db = db().await;
    let combos = ComboRepository::new(db.pool.clone());

    let combo = combos
        .create(CreateCombo {
            name: "chain".to_string(),
            description: None,
            enabled: true,
            entries: vec!["a/one".to_string()],
        })
        .await
        .expect("combo");

    combos.delete(&combo.combo.id).await.expect("delete");

    let orphans: Vec<(i64,)> = sqlx::query_as("SELECT COUNT(*) FROM combo_entries")
        .fetch_all(&db.pool)
        .await
        .expect("count");
    assert_eq!(orphans[0].0, 0);
}

#[tokio::test]
async fn api_key_lookup_by_secret_only_matches_enabled() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let created = repo
        .create(CreateApiKey {
            name: "laptop".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect("key");

    assert!(created.secret.starts_with("sk-router-"));

    let found = repo
        .find_by_secret(&created.secret)
        .await
        .expect("lookup")
        .expect("exists");
    assert_eq!(found.id, created.key.id);

    assert!(
        repo.find_by_secret("sk-router-nope")
            .await
            .expect("lookup")
            .is_none()
    );
}

#[tokio::test]
async fn api_key_secret_is_stored_encrypted() {
    let db = db().await;
    let cipher = test_cipher();
    let repo = ApiKeyRepository::new(db.pool.clone(), cipher.clone());
    let created = repo.create(api_key("k")).await.expect("key");

    assert_ne!(created.key.key_hash, created.secret);
    assert_eq!(created.key.key_hash.len(), 64);

    // The column holds ciphertext, never the key itself.
    let stored: Option<String> = sqlx::query_scalar("SELECT secret_enc FROM api_keys WHERE id = ?")
        .bind(&created.key.id)
        .fetch_one(&db.pool)
        .await
        .expect("raw secret_enc");

    let stored = stored.expect("a stored secret");
    assert!(!stored.contains("sk-router-"));
    assert!(CredentialCipher::is_encrypted(&stored));
    assert_eq!(cipher.decrypt(&stored).expect("decrypt"), created.secret);
}

#[tokio::test]
async fn reveal_secret_returns_the_plaintext() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());
    let created = repo.create(api_key("k")).await.expect("key");

    let revealed = repo
        .reveal_secret(&created.key.id)
        .await
        .expect("reveal")
        .expect("a stored secret");

    assert_eq!(revealed, created.secret);
}

#[tokio::test]
async fn reveal_secret_is_none_when_not_stored() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher()).storing_secrets(false);
    let created = repo.create(api_key("k")).await.expect("key");

    assert!(created.key.secret_enc.is_none());
    assert!(
        repo.reveal_secret(&created.key.id)
            .await
            .expect("reveal")
            .is_none()
    );
    // The key still authenticates: only the reversible copy is missing.
    assert!(
        repo.find_by_secret(&created.secret)
            .await
            .expect("lookup")
            .is_some()
    );
}

#[tokio::test]
async fn reveal_secret_rejects_an_unknown_key() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    assert!(matches!(
        repo.reveal_secret("missing").await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn rotate_replaces_the_secret_and_invalidates_the_old_one() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());
    let created = repo.create(api_key("k")).await.expect("key");

    let rotated = repo.rotate(&created.key.id).await.expect("rotate");

    assert_ne!(rotated.secret, created.secret);
    assert_eq!(rotated.key.id, created.key.id);
    assert_eq!(
        repo.reveal_secret(&created.key.id)
            .await
            .expect("reveal")
            .expect("a stored secret"),
        rotated.secret
    );
    assert!(
        repo.find_by_secret(&created.secret)
            .await
            .expect("lookup")
            .is_none()
    );
    assert!(
        repo.find_by_secret(&rotated.secret)
            .await
            .expect("lookup")
            .is_some()
    );
}

#[tokio::test]
async fn rotate_rejects_an_unknown_key() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    assert!(matches!(
        repo.rotate("missing").await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn usage_filters_narrow_rows_and_summary() {
    let db = db().await;
    let repo = UsageRepository::new(db.pool.clone());

    for (model, provider, connection, cost) in [
        (
            "openai/gpt-4o",
            Some("openai-compatible"),
            "openai-main",
            1.0,
        ),
        (
            "anthropic/claude",
            Some("anthropic-native"),
            "claude-main",
            2.0,
        ),
        (
            "openai/gpt-4o-mini",
            Some("openai-compatible"),
            "openai-main",
            3.0,
        ),
    ] {
        repo.record(NewUsageRecord {
            api_key_id: None,
            requested_model: model.to_string(),
            resolved_provider: provider.map(str::to_string),
            resolved_model: None,
            connection_name: Some(connection.to_string()),
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: cost,
            cost_input_usd: 0.0,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 5,
            ..Default::default()
        })
        .await
        .expect("record");
    }

    let provider_filter =
        UsageFilter::new(None, None, Some("anthropic-native".to_string()), None, None);
    let rows = repo
        .list(10, 0, &provider_filter, Sort::default())
        .await
        .expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].requested_model, "anthropic/claude");
    let summary = repo.summary(&provider_filter).await.expect("summary");
    assert_eq!(summary.requests, 1);
    assert!((summary.cost_usd - 2.0).abs() < f64::EPSILON);

    // Model matching is a case-insensitive substring, so both variants match.
    let model_filter = UsageFilter::new(None, Some("GPT-4O".to_string()), None, None, None);
    let rows = repo
        .list(10, 0, &model_filter, Sort::default())
        .await
        .expect("list");
    assert_eq!(rows.len(), 2);
    let summary = repo.summary(&model_filter).await.expect("summary");
    assert_eq!(summary.requests, 2);

    // The connection filter is an exact name match.
    let connection_filter =
        UsageFilter::new(None, None, None, Some("openai-main".to_string()), None);
    let rows = repo
        .list(10, 0, &connection_filter, Sort::default())
        .await
        .expect("list");
    assert_eq!(rows.len(), 2);
    let summary = repo.summary(&connection_filter).await.expect("summary");
    assert!((summary.cost_usd - 4.0).abs() < f64::EPSILON);

    let facets = repo.facets().await.expect("facets");
    assert_eq!(facets.models.len(), 3);
    assert_eq!(
        facets.providers,
        vec!["anthropic-native", "openai-compatible"]
    );
    assert_eq!(facets.connections, vec!["openai-main", "claude-main"]);
}

#[tokio::test]
async fn blank_usage_filters_count_as_unset() {
    let filter = UsageFilter::new(
        Some("  ".to_string()),
        Some(String::new()),
        Some("\t".to_string()),
        Some(String::new()),
        None,
    );

    assert!(filter.api_key_id.is_none());
    assert!(filter.model.is_none());
    assert!(filter.provider.is_none());
    assert!(filter.connection.is_none());
}

#[tokio::test]
async fn usage_summary_rolls_up_counts_and_cost() {
    let db = db().await;
    let repo = UsageRepository::new(db.pool.clone());

    repo.record(NewUsageRecord {
        api_key_id: None,
        requested_model: "fast".to_string(),
        resolved_provider: Some("openai-compatible".to_string()),
        resolved_model: Some("gpt-4o".to_string()),
        connection_name: None,
        attempt: 1,
        status: "ok".to_string(),
        prompt_tokens: 100,
        completion_tokens: 50,
        cached_tokens: 10,
        reasoning_tokens: 0,
        cost_usd: 0.001,
        cost_input_usd: 0.0,
        cost_output_usd: 0.0,
        cost_reasoning_usd: 0.0,
        latency_ms: 200,
        ..Default::default()
    })
    .await
    .expect("record ok");

    repo.record(NewUsageRecord {
        api_key_id: None,
        requested_model: "fast".to_string(),
        resolved_provider: Some("anthropic-native".to_string()),
        resolved_model: Some("claude".to_string()),
        connection_name: None,
        attempt: 2,
        status: "error".to_string(),
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
        reasoning_tokens: 0,
        cost_usd: 0.0,
        cost_input_usd: 0.0,
        cost_output_usd: 0.0,
        cost_reasoning_usd: 0.0,
        latency_ms: 100,
        ..Default::default()
    })
    .await
    .expect("record error");

    let summary = repo
        .summary(&UsageFilter::default())
        .await
        .expect("summary");
    assert_eq!(summary.requests, 2);
    assert_eq!(summary.ok_requests, 1);
    assert_eq!(summary.error_requests, 1);
    assert_eq!(summary.prompt_tokens, 100);
    assert_eq!(summary.completion_tokens, 50);
    assert_eq!(summary.cached_tokens, 10);
    assert!((summary.cost_usd - 0.001).abs() < 1e-9);
    assert!((summary.avg_latency_ms - 150.0).abs() < 1e-9);
}

#[tokio::test]
async fn usage_savings_round_trip_and_aggregate() -> Result<()> {
    let db = db().await;
    let repo = UsageRepository::new(db.pool.clone());

    repo.record(
        NewUsageRecord {
            requested_model: "fast".to_string(),
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 100,
            completion_tokens: 50,
            latency_ms: 10,
            ..Default::default()
        }
        .with_savings(SavingsTotals {
            saved_rtk_tokens: 400,
            saved_headroom_tokens: 100,
            saved_terse_tokens: 0,
            saved_caveman_tokens: 30,
            saved_ponytail_tokens: 5,
            saved_cost_usd: 0.0024,
        }),
    )
    .await?;

    // A second row with no saver enabled must not inflate the request count.
    repo.record(NewUsageRecord {
        requested_model: "fast".to_string(),
        attempt: 1,
        status: "ok".to_string(),
        prompt_tokens: 10,
        completion_tokens: 5,
        latency_ms: 10,
        ..Default::default()
    })
    .await?;

    let rows = repo
        .list(10, 0, &UsageFilter::default(), Sort::default())
        .await?;
    let saved = rows
        .iter()
        .find(|row| row.saved_rtk_tokens > 0)
        .expect("row with savings");
    assert_eq!(saved.saved_rtk_tokens, 400);
    assert_eq!(saved.saved_headroom_tokens, 100);
    assert_eq!(saved.saved_caveman_tokens, 30);
    assert_eq!(saved.saved_ponytail_tokens, 5);
    assert_eq!(saved.saved_terse_tokens, 0);
    assert!((saved.saved_cost_usd - 0.0024).abs() < 1e-9);

    let savings = repo.savings(&UsageFilter::default()).await?;
    assert_eq!(
        savings.requests, 1,
        "only the row that saved anything counts"
    );
    assert_eq!(savings.measured_tokens(), 500);
    assert_eq!(savings.estimated_tokens(), 35);
    assert_eq!(savings.saved_tokens(), 535);
    assert_eq!(
        savings.contributions(),
        vec![
            ("rtk", 400),
            ("headroom", 100),
            ("caveman", 30),
            ("ponytail", 5)
        ]
    );

    let summary = repo.summary(&UsageFilter::default()).await?;
    let combined = summary.savings(savings);
    assert_eq!(combined.savings.saved_rtk_tokens, 400);
    assert_eq!(combined.requests, 2);

    Ok(())
}

#[tokio::test]
async fn usage_list_returns_newest_first() -> Result<()> {
    let db = db().await;
    let repo = UsageRepository::new(db.pool.clone());

    for model in ["first", "second"] {
        repo.record(NewUsageRecord {
            api_key_id: None,
            requested_model: model.to_string(),
            resolved_provider: None,
            resolved_model: None,
            connection_name: None,
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: 0.0,
            cost_input_usd: 0.0,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 10,
            ..Default::default()
        })
        .await?;
    }

    let rows = repo
        .list(10, 0, &UsageFilter::default(), Sort::default())
        .await?;
    assert_eq!(rows.len(), 2);
    assert!(rows[0].created_at >= rows[1].created_at);
    Ok(())
}

#[tokio::test]
async fn api_key_limits_and_budget_round_trip() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let expires_at = chrono::Utc::now() + chrono::Duration::days(30);
    let created = repo
        .create(CreateApiKey {
            name: "metered".to_string(),
            enabled: true,
            rate_limit_per_minute: Some(30),
            daily_budget_usd: Some(1.5),
            weekly_budget_usd: Some(7.0),
            monthly_budget_usd: Some(5.0),
            lifetime_budget_usd: Some(50.0),
            daily_token_limit: Some(1_000_000),
            weekly_token_limit: Some(5_000_000),
            monthly_token_limit: Some(20_000_000),
            lifetime_token_limit: Some(200_000_000),
            budget_mode: Some("warn".to_string()),
            plan_id: None,
            allowed_models: None,
            expires_at: Some(expires_at),
        })
        .await
        .expect("create");

    assert_eq!(created.key.rate_limit(), Some(30));
    assert_eq!(created.key.budget_windows().daily, Some(1.5));
    assert_eq!(created.key.budget_windows().weekly, Some(7.0));
    assert_eq!(created.key.budget_windows().monthly, Some(5.0));
    assert_eq!(created.key.budget_windows().lifetime, Some(50.0));
    assert_eq!(created.key.token_windows().daily, Some(1_000_000));
    assert_eq!(created.key.token_windows().weekly, Some(5_000_000));
    assert_eq!(created.key.token_windows().monthly, Some(20_000_000));
    assert_eq!(created.key.token_windows().lifetime, Some(200_000_000));
    assert_eq!(created.key.budget_mode(), BudgetMode::Warn);
    assert_eq!(created.key.expires_at, Some(expires_at));
    assert!(!created.key.is_expired());

    let updated = repo
        .update(
            &created.key.id,
            UpdateApiKey {
                enabled: Some(false),
                rate_limit_per_minute: Some(None),
                daily_budget_usd: Some(None),
                weekly_budget_usd: Some(None),
                monthly_budget_usd: Some(None),
                lifetime_budget_usd: Some(None),
                daily_token_limit: Some(None),
                weekly_token_limit: Some(None),
                monthly_token_limit: Some(None),
                lifetime_token_limit: Some(None),
                budget_mode: Some("off".to_string()),
                expires_at: Some(None),
                ..Default::default()
            },
        )
        .await
        .expect("update");

    assert!(!updated.is_enabled());
    assert_eq!(updated.rate_limit(), None);
    assert!(updated.budget_windows().is_empty());
    assert!(updated.token_windows().is_empty());
    assert_eq!(updated.budget_mode(), BudgetMode::Off);
    assert_eq!(updated.expires_at, None);
}

#[tokio::test]
async fn expired_keys_are_detected() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let expired = repo
        .create(CreateApiKey {
            name: "stale".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: Some(chrono::Utc::now() - chrono::Duration::seconds(1)),
        })
        .await
        .expect("create");

    assert!(expired.key.is_expired());
}

#[tokio::test]
async fn api_key_budget_mode_requires_a_budget() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let error = repo
        .create(CreateApiKey {
            name: "bad".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: Some("block".to_string()),
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect_err("budget mode without a budget must be refused");

    assert!(matches!(error, Error::BadRequest(_)));
}

#[tokio::test]
async fn a_single_window_satisfies_the_budget_mode_pair() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let created = repo
        .create(CreateApiKey {
            name: "daily-only".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: Some(2.0),
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: Some("block".to_string()),
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect("a daily cap alone is enough for warn or block");

    assert_eq!(created.key.budget_windows().daily, Some(2.0));
    assert_eq!(created.key.budget_windows().monthly, None);
}

#[tokio::test]
async fn usage_spend_since_sums_only_the_matching_key() {
    let db = db().await;
    let usage = UsageRepository::new(db.pool.clone());
    let keys = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let create_key = |name: &str| CreateApiKey {
        name: name.to_string(),
        enabled: true,
        rate_limit_per_minute: None,
        daily_budget_usd: None,
        weekly_budget_usd: None,
        monthly_budget_usd: None,
        lifetime_budget_usd: None,
        daily_token_limit: None,
        weekly_token_limit: None,
        monthly_token_limit: None,
        lifetime_token_limit: None,
        budget_mode: None,
        plan_id: None,
        allowed_models: None,
        expires_at: None,
    };

    let key = keys.create(create_key("metered")).await.expect("key");
    let other = keys.create(create_key("other")).await.expect("key");

    for (api_key_id, cost) in [
        (Some(key.key.id.clone()), 1.5),
        (Some(other.key.id.clone()), 9.9),
    ] {
        usage
            .record(NewUsageRecord {
                api_key_id,
                requested_model: "m".to_string(),
                resolved_provider: None,
                resolved_model: None,
                connection_name: None,
                attempt: 1,
                status: "ok".to_string(),
                prompt_tokens: 1,
                completion_tokens: 1,
                cached_tokens: 0,
                reasoning_tokens: 0,
                cost_usd: cost,
                cost_input_usd: 0.0,
                cost_output_usd: 0.0,
                cost_reasoning_usd: 0.0,
                latency_ms: 10,
                ..Default::default()
            })
            .await
            .expect("record");
    }

    let since = chrono::Utc::now() - chrono::Duration::hours(1);
    let spent = usage.spend_since(&key.key.id, since).await.expect("spend");

    assert!((spent - 1.5).abs() < 1e-9, "unexpected spend: {spent}");
}

#[tokio::test]
async fn spend_by_key_splits_windows_and_skips_null_keys() {
    let db = db().await;
    let usage = UsageRepository::new(db.pool.clone());
    let keys = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let create_key = |name: &str| CreateApiKey {
        name: name.to_string(),
        enabled: true,
        rate_limit_per_minute: None,
        daily_budget_usd: None,
        weekly_budget_usd: None,
        monthly_budget_usd: None,
        lifetime_budget_usd: None,
        daily_token_limit: None,
        weekly_token_limit: None,
        monthly_token_limit: None,
        lifetime_token_limit: None,
        budget_mode: None,
        plan_id: None,
        allowed_models: None,
        expires_at: None,
    };

    let key = keys.create(create_key("metered")).await.expect("key");
    let other = keys.create(create_key("other")).await.expect("key");

    for (api_key_id, cost) in [
        (Some(key.key.id.clone()), 1.0),
        (Some(other.key.id.clone()), 2.0),
        (None, 5.0),
    ] {
        usage
            .record(NewUsageRecord {
                api_key_id,
                requested_model: "m".to_string(),
                resolved_provider: None,
                resolved_model: None,
                connection_name: None,
                attempt: 1,
                status: "ok".to_string(),
                prompt_tokens: 1,
                completion_tokens: 1,
                cached_tokens: 0,
                reasoning_tokens: 0,
                cost_usd: cost,
                cost_input_usd: 0.0,
                cost_output_usd: 0.0,
                cost_reasoning_usd: 0.0,
                latency_ms: 10,
                ..Default::default()
            })
            .await
            .expect("record");
    }

    // A row from a previous month counts toward the lifetime total only.
    let old = chrono::Utc::now() - chrono::Duration::days(40);
    sqlx::query(
        "INSERT INTO usage_records (id, created_at, api_key_id, requested_model, status, cost_usd)
         VALUES (?, ?, ?, 'old', 'ok', 3.0)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(old)
    .bind(&key.key.id)
    .execute(&db.pool)
    .await
    .expect("insert old row");

    let now = chrono::Utc::now();
    let rows = usage
        .spend_by_key(
            BudgetWindow::Daily.start(now),
            BudgetWindow::Weekly.start(now),
            BudgetWindow::Monthly.start(now),
        )
        .await
        .expect("spend");

    assert_eq!(rows.len(), 2, "null-key rows are skipped: {rows:?}");

    let first = rows
        .iter()
        .find(|row| row.api_key_id == key.key.id)
        .expect("key row");
    assert!((first.daily_usd - 1.0).abs() < 1e-9);
    assert!((first.weekly_usd - 1.0).abs() < 1e-9);
    assert!((first.monthly_usd - 1.0).abs() < 1e-9);
    assert!((first.lifetime_usd - 4.0).abs() < 1e-9);
    // Each recorded row carries 1 prompt + 1 completion token.
    assert_eq!(first.daily_tokens, 2);
    assert_eq!(first.weekly_tokens, 2);
    assert_eq!(first.monthly_tokens, 2);
    assert_eq!(first.lifetime_tokens, 2);

    let second = rows
        .iter()
        .find(|row| row.api_key_id == other.key.id)
        .expect("other row");
    assert!((second.lifetime_usd - 2.0).abs() < 1e-9);
    assert_eq!(second.lifetime_tokens, 2);
}

#[tokio::test]
async fn spend_for_key_returns_zeroed_row_for_unused_key() {
    let db = db().await;
    let usage = UsageRepository::new(db.pool.clone());
    let keys = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let key = keys
        .create(CreateApiKey {
            name: "fresh".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect("key");

    let now = chrono::Utc::now();
    let spend = usage
        .spend_for_key(
            &key.key.id,
            BudgetWindow::Daily.start(now),
            BudgetWindow::Weekly.start(now),
            BudgetWindow::Monthly.start(now),
        )
        .await
        .expect("spend");

    assert_eq!(spend.api_key_id, key.key.id);
    assert!((spend.daily_usd).abs() < 1e-9);
    assert!((spend.weekly_usd).abs() < 1e-9);
    assert!((spend.monthly_usd).abs() < 1e-9);
    assert!((spend.lifetime_usd).abs() < 1e-9);
    assert_eq!(spend.daily_tokens, 0);
    assert_eq!(spend.weekly_tokens, 0);
    assert_eq!(spend.monthly_tokens, 0);
    assert_eq!(spend.lifetime_tokens, 0);
}

#[tokio::test]
async fn spend_for_key_splits_windows_for_one_key() {
    let db = db().await;
    let usage = UsageRepository::new(db.pool.clone());
    let keys = ApiKeyRepository::new(db.pool.clone(), test_cipher());

    let key = keys
        .create(CreateApiKey {
            name: "metered".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect("key");

    // Recent row counts toward all windows.
    usage
        .record(NewUsageRecord {
            api_key_id: Some(key.key.id.clone()),
            requested_model: "m".to_string(),
            resolved_provider: None,
            resolved_model: None,
            connection_name: None,
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 10,
            completion_tokens: 5,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: 1.0,
            cost_input_usd: 0.0,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 10,
            ..Default::default()
        })
        .await
        .expect("record");

    // Old row counts toward lifetime only.
    let old = chrono::Utc::now() - chrono::Duration::days(40);
    sqlx::query(
        "INSERT INTO usage_records (id, created_at, api_key_id, requested_model, status, cost_usd,
         prompt_tokens, completion_tokens)
         VALUES (?, ?, ?, 'old', 'ok', 3.0, 4, 6)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(old)
    .bind(&key.key.id)
    .execute(&db.pool)
    .await
    .expect("insert old row");

    let now = chrono::Utc::now();
    let spend = usage
        .spend_for_key(
            &key.key.id,
            BudgetWindow::Daily.start(now),
            BudgetWindow::Weekly.start(now),
            BudgetWindow::Monthly.start(now),
        )
        .await
        .expect("spend");

    assert_eq!(spend.api_key_id, key.key.id);
    assert!((spend.daily_usd - 1.0).abs() < 1e-9);
    assert!((spend.weekly_usd - 1.0).abs() < 1e-9);
    assert!((spend.monthly_usd - 1.0).abs() < 1e-9);
    assert!((spend.lifetime_usd - 4.0).abs() < 1e-9);
    assert_eq!(spend.daily_tokens, 15);
    assert_eq!(spend.weekly_tokens, 15);
    assert_eq!(spend.monthly_tokens, 15);
    assert_eq!(spend.lifetime_tokens, 15 + 10);
}

#[tokio::test]
async fn pricing_overrides_shadow_synced_rows() {
    let db = db().await;
    let repo = PricingRepository::new(db.pool.clone());
    let cache = PricingCache::new(db.pool.clone());

    repo.replace_synced(
        &[
            FetchedPrice {
                model: "gpt-4o".to_string(),
                price: synced_price(2.5, 10.0),
            },
            FetchedPrice {
                model: "claude-sonnet-4-5".to_string(),
                price: synced_price(3.0, 15.0),
            },
        ],
        chrono::Utc::now(),
    )
    .await
    .expect("seed sync");

    repo.upsert_overrides(&[PriceInput {
        model: "gpt-4o".to_string(),
        input_per_million_usd: 1.0,
        output_per_million_usd: 2.0,
        cache_read_per_million_usd: Some(0.5),
        cache_write_per_million_usd: None,
        reasoning_per_million_usd: Some(3.0),
    }])
    .await
    .expect("override");

    let rows = repo.list().await.expect("list");
    assert_eq!(rows.len(), 3, "both sources stay stored");

    // The override wins in the cache, and lookups fall back to the leaf id.
    let override_price = cache.price_for("gpt-4o").await.expect("price");
    assert_eq!(override_price.input_per_million_usd, 1.0);
    assert_eq!(override_price.reasoning_per_million_usd, Some(3.0));
    let prefixed = cache.price_for("openai/gpt-4o").await.expect("prefixed");
    assert_eq!(prefixed.input_per_million_usd, 1.0);
    let synced = cache.price_for("claude-sonnet-4-5").await.expect("synced");
    assert_eq!(synced.input_per_million_usd, 3.0);

    // Deleting the override falls back to the synced row after invalidation.
    assert_eq!(
        repo.delete_overrides(Some("gpt-4o")).await.expect("delete"),
        1
    );
    cache.invalidate().await;
    let fallback = cache.price_for("gpt-4o").await.expect("fallback");
    assert_eq!(fallback.input_per_million_usd, 2.5);
    assert_eq!(fallback.reasoning_per_million_usd, None);
}

#[tokio::test]
async fn pricing_sync_runs_keep_the_latest_status() {
    let db = db().await;
    let repo = PricingRepository::new(db.pool.clone());

    assert!(repo.sync_status().await.expect("status").is_none());

    for count in [3, 7] {
        repo.record_sync(&PricingSyncStatus {
            source: "https://example.com/pricing.json".to_string(),
            synced_at: chrono::Utc::now(),
            model_count: count,
        })
        .await
        .expect("record");
    }

    let status = repo.sync_status().await.expect("status").expect("run");
    assert_eq!(status.model_count, 7);
    assert_eq!(status.source, "https://example.com/pricing.json");
}

fn synced_price(input: f64, output: f64) -> Price {
    Price {
        input_per_million_usd: input,
        output_per_million_usd: output,
        cache_read_per_million_usd: None,
        cache_write_per_million_usd: None,
        reasoning_per_million_usd: None,
    }
}

#[tokio::test]
async fn pricing_matches_by_leaf_and_prefers_canonical_rows() {
    let db = db().await;
    let repo = PricingRepository::new(db.pool.clone());
    let cache = PricingCache::new(db.pool.clone());

    repo.replace_synced(
        &[
            FetchedPrice {
                model: "azure/gpt-5.6-luna".to_string(),
                price: synced_price(3.0, 12.0),
            },
            FetchedPrice {
                model: "azure/eu/gpt-5.6-luna".to_string(),
                price: synced_price(4.0, 16.0),
            },
        ],
        chrono::Utc::now(),
    )
    .await
    .expect("seed sync");

    // A bare or relayed id resolves to the cheapest prefixed variant.
    let prefixed = cache.match_for("gpt-5.6-luna").await.expect("prefixed");
    assert_eq!(prefixed.matched, "azure/gpt-5.6-luna");
    let relayed = cache
        .match_for("ocg/openai/gpt-5.6-luna")
        .await
        .expect("relay");
    assert_eq!(relayed.matched, "azure/gpt-5.6-luna");

    // A canonical row outranks prefixed ones even when it is pricier.
    repo.replace_synced(
        &[
            FetchedPrice {
                model: "azure/gpt-5.6-luna".to_string(),
                price: synced_price(3.0, 12.0),
            },
            FetchedPrice {
                model: "gpt-5.6-luna".to_string(),
                price: synced_price(3.5, 14.0),
            },
        ],
        chrono::Utc::now(),
    )
    .await
    .expect("reseed");
    cache.invalidate().await;

    let canonical = cache.match_for("gpt-5.6-luna").await.expect("canonical");
    assert_eq!(canonical.matched, "gpt-5.6-luna");
    assert_eq!(canonical.price.input_per_million_usd, 3.5);
}

#[tokio::test]
async fn connections_round_trip_a_pricing_model_pin() {
    let db = db().await;
    let repo = connection_repo(&db);

    let created = repo
        .create(CreateConnection {
            name: "relay".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url: "https://relay.example.com/v1".to_string(),
            api_key: None,
            custom_headers: Default::default(),
            enabled: true,
            connect_timeout_ms: None,
            idle_timeout_ms: None,
            pricing_model: Some("gpt-5.6-luna".to_string()),
            provider_id: None,
        })
        .await
        .expect("create");
    assert_eq!(created.pricing_model.as_deref(), Some("gpt-5.6-luna"));

    let cleared = repo
        .update(
            &created.id,
            UpdateConnection {
                pricing_model: Some(None),
                ..Default::default()
            },
        )
        .await
        .expect("clear");
    assert!(cleared.pricing_model.is_none());
}

#[tokio::test]
async fn usage_cost_breakdown_round_trips_and_sums() {
    let db = db().await;
    let repo = UsageRepository::new(db.pool.clone());

    repo.record(NewUsageRecord {
        api_key_id: None,
        requested_model: "priced".to_string(),
        resolved_provider: Some("openai-compatible".to_string()),
        resolved_model: Some("gpt-4o".to_string()),
        connection_name: Some("openai-main".to_string()),
        attempt: 1,
        status: "ok".to_string(),
        prompt_tokens: 100,
        completion_tokens: 50,
        cached_tokens: 20,
        reasoning_tokens: 10,
        cost_usd: 3.0,
        cost_input_usd: 1.0,
        cost_output_usd: 1.5,
        cost_reasoning_usd: 0.5,
        latency_ms: 10,
        ..Default::default()
    })
    .await
    .expect("record");

    let rows = repo
        .list(10, 0, &UsageFilter::default(), Sort::default())
        .await
        .expect("list");
    assert_eq!(rows[0].cost_input_usd, 1.0);
    assert_eq!(rows[0].cost_output_usd, 1.5);
    assert_eq!(rows[0].cost_reasoning_usd, 0.5);
    assert_eq!(rows[0].reasoning_tokens, 10);

    let summary = repo
        .summary(&UsageFilter::default())
        .await
        .expect("summary");
    assert!((summary.cost_usd - 3.0).abs() < f64::EPSILON);
    assert!((summary.cost_input_usd - 1.0).abs() < f64::EPSILON);
    assert!((summary.cost_output_usd - 1.5).abs() < f64::EPSILON);
    assert!((summary.cost_reasoning_usd - 0.5).abs() < f64::EPSILON);
    assert_eq!(summary.reasoning_tokens, 10);
}
