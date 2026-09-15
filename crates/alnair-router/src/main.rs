//! Binary entrypoint for the standalone alnair-router server.
//!
//! On Windows this is a GUI-subsystem app so auto-start and double-click never
//! pop a console window; [`bind_parent_console`] re-attaches stdio when the
//! binary is invoked from a terminal so CLI output and logs stay visible.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::sync::Arc;

use alnair_router::config::RouterConfig;
use alnair_router::{Db, Error, Result, build_router, cli, config, state::AppState};
use tokio::sync::Notify;

fn main() -> Result<()> {
    #[cfg(windows)]
    bind_parent_console();

    match cli::Command::parse(std::env::args())? {
        cli::Command::Serve { tray } => serve_command(tray),
        cli::Command::Install => cli::install(),
        cli::Command::Uninstall => cli::uninstall(),
        cli::Command::Status => cli::status(),
        cli::Command::Help => {
            println!("{}", cli::help());
            Ok(())
        }
        cli::Command::Version => {
            println!("alnair-router {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

/// Re-attaches stdio to the launching terminal, when there is one.
///
/// GUI-subsystem processes get no console, so `println!` and logs would vanish
/// when the binary is run from a shell. Attaching to the parent console (and
/// reopening `CONOUT$`/`CONIN$`) restores them; handles Windows already
/// provided — e.g. redirected output — are left alone.
#[cfg(windows)]
fn bind_parent_console() {
    use std::ptr;

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE, SetStdHandle,
    };

    fn has_handle(std: u32) -> bool {
        let handle = unsafe { GetStdHandle(std) };
        !handle.is_null() && handle != INVALID_HANDLE_VALUE
    }

    fn bind(name: &str, access: u32, std: u32) {
        let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            unsafe { SetStdHandle(std, handle) };
        }
    }

    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }

    if !has_handle(STD_OUTPUT_HANDLE) {
        bind(
            "CONOUT$",
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            STD_OUTPUT_HANDLE,
        );
    }
    if !has_handle(STD_ERROR_HANDLE) {
        bind(
            "CONOUT$",
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            STD_ERROR_HANDLE,
        );
    }
    if !has_handle(STD_INPUT_HANDLE) {
        bind(
            "CONIN$",
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            STD_INPUT_HANDLE,
        );
    }
}
/// Loads the config, then serves with or without the tray icon.
fn serve_command(tray: Option<bool>) -> Result<()> {
    init_tracing();
    let config = config::load()?;

    if tray_enabled(tray, &config) {
        run_with_tray(config)
    } else {
        run_without_tray(config)
    }
}

/// True when the tray should run: the flag wins over `server.tray`.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn tray_enabled(flag: Option<bool>, config: &RouterConfig) -> bool {
    flag.unwrap_or(config.server.tray)
}

/// Platforms without tray support always serve headless.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn tray_enabled(flag: Option<bool>, _config: &RouterConfig) -> bool {
    if flag == Some(true) {
        tracing::warn!(
            "the system tray is only available on Windows and macOS; serving without it"
        );
    }
    false
}

/// Runs the server on the current thread.
fn run_without_tray(config: RouterConfig) -> Result<()> {
    runtime()?.block_on(serve(config, Arc::new(Notify::new())))
}

/// Runs the tray event loop on this (main) thread and the server on a worker.
///
/// The tray has to own the main thread: `tao` requires an event loop there and
/// macOS additionally wants it running before the icon is created.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn run_with_tray(config: RouterConfig) -> Result<()> {
    let address = format!("{}:{}", config.server.host, config.server.port);
    let shutdown = Arc::new(Notify::new());
    let (done_tx, done_rx) = std::sync::mpsc::channel();

    let server = std::thread::Builder::new()
        .name("alnair-http".to_string())
        .spawn({
            let shutdown = shutdown.clone();
            move || {
                let result = runtime()?.block_on(serve(config, shutdown));
                let _ = done_tx.send(());
                result
            }
        })
        .map_err(|error| Error::Internal(format!("cannot spawn the server thread: {error}")))?;

    alnair_router::desktop::run(alnair_router::desktop::Options {
        address,
        shutdown,
        server_done: done_rx,
    })?;

    match server.join() {
        Ok(result) => result,
        Err(_) => Err(Error::Internal("the server thread panicked".to_string())),
    }
}

/// Builds the multi-threaded async runtime.
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new()
        .map_err(|error| Error::Internal(format!("cannot start the async runtime: {error}")))
}

/// Initializes the tracing subscriber from `RUST_LOG` (default `info`).
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

/// Runs the HTTP server until a termination signal arrives.
async fn serve(config: RouterConfig, shutdown: Arc<Notify>) -> Result<()> {
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
        .map_err(|error| Error::Config(format!("cannot bind {address}: {error}")))?;

    tracing::info!(
        address = %address,
        require_api_key = config.server.require_api_key,
        admin_token = config.server.requires_admin_token(),
        "alnair-router listening"
    );

    let app = build_router(AppState::new(config, db)?);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown))
        .await
        .map_err(|error| Error::Internal(error.to_string()))?;

    Ok(())
}

/// Resolves on Ctrl+C, SIGTERM, or a tray "Quit".
async fn shutdown_signal(shutdown: Arc<Notify>) {
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
        _ = shutdown.notified() => {},
    }

    tracing::info!("shutdown signal received");
}
