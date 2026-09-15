//! Binary entrypoint for the standalone alnair-router server.

use alnair_router::{Db, Result, build_router, config, state::AppState};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = config::load()?;
    let cipher = alnair_router::crypto::CredentialCipher::from_config(&config.secrets)?;
    let db = Db::connect(&config).await?;
    db.migrate().await?;

    let re_encrypted = db.migrate_credentials(&cipher).await?;
    if re_encrypted > 0 {
        tracing::info!(
            count = re_encrypted,
            "encrypted legacy plaintext connection credentials"
        );
    }

    let address = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|error| alnair_router::Error::Config(format!("cannot bind {address}: {error}")))?;

    tracing::info!(
        address = %address,
        require_api_key = config.server.require_api_key,
        admin_token = config.server.requires_admin_token(),
        "alnair-router listening"
    );

    let app = build_router(AppState::new(config, db)?);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| alnair_router::Error::Internal(error.to_string()))?;

    Ok(())
}

/// Resolves when the process receives a termination signal.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}
