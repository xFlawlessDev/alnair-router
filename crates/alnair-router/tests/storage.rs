//! Storage layer tests against an in-memory SQLite database.

use alnair_router::crypto::CredentialCipher;
use alnair_router::db::Db;
use alnair_router::db::repos::aliases::{AliasRepository, CreateAlias};
use alnair_router::db::repos::api_keys::{ApiKeyRepository, CreateApiKey, UpdateApiKey};
use alnair_router::db::repos::combos::{ComboRepository, CreateCombo};
use alnair_router::db::repos::connections::{ConnectionRepository, CreateConnection};
use alnair_router::db::repos::usage::{NewUsageRecord, UsageRepository};
use alnair_router::limits::BudgetMode;
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
    let repo = ApiKeyRepository::new(db.pool.clone());

    let created = repo
        .create(CreateApiKey {
            name: "laptop".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            monthly_budget_usd: None,
            budget_mode: None,
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
async fn api_key_secret_is_never_stored_plaintext() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone());
    let created = repo
        .create(CreateApiKey {
            name: "k".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            monthly_budget_usd: None,
            budget_mode: None,
        })
        .await
        .expect("key");

    assert_ne!(created.key.key_hash, created.secret);
    assert_eq!(created.key.key_hash.len(), 64);
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
        attempt: 1,
        status: "ok".to_string(),
        prompt_tokens: 100,
        completion_tokens: 50,
        cached_tokens: 10,
        cost_usd: 0.001,
        latency_ms: 200,
    })
    .await
    .expect("record ok");

    repo.record(NewUsageRecord {
        api_key_id: None,
        requested_model: "fast".to_string(),
        resolved_provider: Some("anthropic-native".to_string()),
        resolved_model: Some("claude".to_string()),
        attempt: 2,
        status: "error".to_string(),
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
        cost_usd: 0.0,
        latency_ms: 100,
    })
    .await
    .expect("record error");

    let summary = repo.summary(None).await.expect("summary");
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
async fn usage_list_returns_newest_first() -> Result<()> {
    let db = db().await;
    let repo = UsageRepository::new(db.pool.clone());

    for model in ["first", "second"] {
        repo.record(NewUsageRecord {
            api_key_id: None,
            requested_model: model.to_string(),
            resolved_provider: None,
            resolved_model: None,
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            cost_usd: 0.0,
            latency_ms: 10,
        })
        .await?;
    }

    let rows = repo.list(10, 0).await?;
    assert_eq!(rows.len(), 2);
    assert!(rows[0].created_at >= rows[1].created_at);
    Ok(())
}

#[tokio::test]
async fn api_key_limits_and_budget_round_trip() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone());

    let created = repo
        .create(CreateApiKey {
            name: "metered".to_string(),
            enabled: true,
            rate_limit_per_minute: Some(30),
            monthly_budget_usd: Some(5.0),
            budget_mode: Some("warn".to_string()),
        })
        .await
        .expect("create");

    assert_eq!(created.key.rate_limit(), Some(30));
    assert_eq!(created.key.budget_usd(), Some(5.0));
    assert_eq!(created.key.budget_mode(), BudgetMode::Warn);

    let updated = repo
        .update(
            &created.key.id,
            UpdateApiKey {
                enabled: Some(false),
                rate_limit_per_minute: Some(None),
                monthly_budget_usd: Some(None),
                budget_mode: Some("off".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("update");

    assert!(!updated.is_enabled());
    assert_eq!(updated.rate_limit(), None);
    assert_eq!(updated.budget_usd(), None);
    assert_eq!(updated.budget_mode(), BudgetMode::Off);
}

#[tokio::test]
async fn api_key_budget_mode_requires_a_budget() {
    let db = db().await;
    let repo = ApiKeyRepository::new(db.pool.clone());

    let error = repo
        .create(CreateApiKey {
            name: "bad".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            monthly_budget_usd: None,
            budget_mode: Some("block".to_string()),
        })
        .await
        .expect_err("budget mode without a budget must be refused");

    assert!(matches!(error, Error::BadRequest(_)));
}

#[tokio::test]
async fn usage_spend_since_sums_only_the_matching_key() {
    let db = db().await;
    let usage = UsageRepository::new(db.pool.clone());
    let keys = ApiKeyRepository::new(db.pool.clone());

    let create_key = |name: &str| CreateApiKey {
        name: name.to_string(),
        enabled: true,
        rate_limit_per_minute: None,
        monthly_budget_usd: None,
        budget_mode: None,
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
                attempt: 1,
                status: "ok".to_string(),
                prompt_tokens: 1,
                completion_tokens: 1,
                cached_tokens: 0,
                cost_usd: cost,
                latency_ms: 10,
            })
            .await
            .expect("record");
    }

    let since = chrono::Utc::now() - chrono::Duration::hours(1);
    let spent = usage.spend_since(&key.key.id, since).await.expect("spend");

    assert!((spent - 1.5).abs() < 1e-9, "unexpected spend: {spent}");
}
