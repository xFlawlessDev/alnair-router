//! Binary entrypoint for the standalone alnair-router server.
//!
//! On Windows this is a GUI-subsystem app so auto-start and double-click never
//! pop a console window; [`bind_parent_console`] re-attaches stdio when the
//! binary is invoked from a terminal so CLI output and logs stay visible.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(unix)]
use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use alnair_router::config::RouterConfig;
use alnair_router::{Db, Error, Result, build_router, cli, state::AppState};
use tokio::sync::Notify;

fn main() -> Result<()> {
    #[cfg(windows)]
    let console_attached = bind_parent_console();
    #[cfg(unix)]
    let console_attached = std::io::stdout().is_terminal();

    match cli::Command::parse(std::env::args())? {
        cli::Command::Serve { tray, mode, port } => {
            serve_command(tray, mode, port, console_attached)
        }
        cli::Command::Start { tray, port } => cli::daemon::start(tray, port),
        cli::Command::Stop { force } => cli::daemon::stop(force),
        cli::Command::Restart { force, port } => cli::daemon::restart(force, port),
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
///
/// Returns whether a console was attached, which is also how the caller knows a
/// terminal is waiting: with one, `serve` detaches instead of blocking it.
///
/// A process `start` spawned in the background is never re-attached. It was
/// created `DETACHED_PROCESS` to outlive the terminal, and its parent — the CLI
/// that launched it — still owns that terminal's console, so `AttachConsole`
/// would succeed and quietly undo the detach. Closing the terminal would then
/// take the router down with it, which is exactly what detaching prevents.
#[cfg(windows)]
fn bind_parent_console() -> bool {
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

    if cli::daemon::detached_requested() {
        return false;
    }

    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return false;
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

    true
}

/// Serves in this process, or detaches when a terminal is waiting.
///
/// `console_attached` reports whether the router was launched from a terminal.
/// From one, serving here would block that terminal until the process exits, so
/// the CLI hands off to a detached child and returns; auto-start, double-click
/// and container launches have no terminal and keep serving in-process.
fn serve_command(
    tray: Option<bool>,
    mode: cli::ServeMode,
    port: Option<u16>,
    console_attached: bool,
) -> Result<()> {
    // ALNAIR_ROUTER_FOREGROUND=1 is the scriptable form of `--foreground`.
    if !cli::daemon::foreground_requested() && cli::daemon::should_detach(mode, console_attached) {
        return cli::daemon::start(tray, port);
    }

    init_tracing();
    let config = cli::daemon::load_config(port)?;

    if tray_enabled(tray, &config) {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        return run_with_tray(config);
    }

    run_without_tray(config)
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
    let address = config.server.browser_address();
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

    let state = AppState::new(config, db)?;
    // The tray's Quit item and the local control endpoint notify the same
    // signal, so both stop the server through one graceful path.
    let shutdown = state.register_shutdown(shutdown);
    // The CLI's stop/status need the control token; generating it here keeps
    // this process the single writer.
    cli::daemon::ensure_control_token()?;

    // Dashboard overrides win over file/env values and persist in the database.
    let overrides = state.settings().get().await?;
    if !overrides.is_empty() {
        state.apply_overrides(&overrides).await.map_err(|error| {
            Error::Config(format!(
                "stored dashboard settings are invalid: {error}. \
                 Fix them in the `settings` table or reset them from the dashboard."
            ))
        })?;
        tracing::info!(keys = ?overrides.keys(), "applied dashboard settings");
    }

    if let Some(code) = state.init_setup_code().await? {
        tracing::warn!(
            setup_code = %code,
            "no dashboard password yet; set one at /login with this setup code"
        );
        println!("alnair-router setup code: {code}");
    }

    spawn_pricing_sync(&state);
    let app = build_router(state.clone());

    // The loops are up, so dashboard saves may wake them from here on. The
    // overrides applied above were startup state, not changes: notifying then
    // would leave a permit the serving loop consumes immediately.
    state.start_loops();

    // Record this process and the address it serves on, for `stop`/`status`.
    // `browser_address` is the loopback-reachable form of the bind, which is
    // where the CLI sends its control requests. The guard removes the file on
    // every exit path, including a panic.
    let address = state.config_snapshot().server.browser_address();
    let _pid_file = cli::daemon::PidFile::write(&address)?;

    // The listener re-binds in place when LAN access or the port changes; the
    // rest of the process keeps running. A `stop` that arrived before this
    // point left a permit on `shutdown`, so the first wait stops immediately
    // rather than serving on.
    loop {
        let effective = state.config_snapshot();
        let address = effective.server.listen_address();
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|error| Error::Config(format!("cannot bind {address}: {error}")))?;

        tracing::info!(
            address = %effective.server.browser_url(),
            require_api_key = effective.server.require_api_key,
            admin_token = effective.server.requires_admin_token(),
            lan_access = effective.server.lan_access,
            "alnair-router listening"
        );

        let stopping = Arc::new(AtomicBool::new(false));
        let signal = {
            let shutdown = shutdown.clone();
            let rebind = state.rebind.clone();
            let stopping = stopping.clone();
            async move {
                if shutdown_signal(shutdown, rebind).await {
                    return;
                }
                // A rebind, not a stop: tell the loop to bind again in place.
                stopping.store(true, Ordering::SeqCst);
            }
        };

        axum::serve(listener, app.clone())
            .with_graceful_shutdown(signal)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?;

        if !stopping.load(Ordering::SeqCst) {
            break;
        }
        tracing::info!("re-binding the listener after a settings change");
    }

    Ok(())
}

/// Crawls the pricing catalog in the background while `pricing.sync_enabled`.
///
/// The loop re-reads the live configuration each iteration, so enabling or
/// reconfiguring sync from the dashboard takes effect without a restart; the
/// settings handler wakes it through `pricing_sync_trigger`.
fn spawn_pricing_sync(state: &AppState) {
    let state = state.clone();

    tokio::spawn(async move {
        loop {
            let pricing = state.config_snapshot().pricing;
            if !pricing.sync_enabled {
                state.pricing_sync_trigger.notified().await;
                continue;
            }

            match alnair_router::pricing::sync_from_source(
                &state.pricing_cache,
                &pricing.source_url,
            )
            .await
            {
                Ok(status) => tracing::info!(
                    source = %status.source,
                    models = status.model_count,
                    "pricing catalog synced"
                ),
                Err(error) => tracing::warn!(%error, "pricing sync failed"),
            }

            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(
                    pricing.sync_interval_secs.max(60),
                )) => {},
                _ = state.pricing_sync_trigger.notified() => {},
            }
        }
    });
}

/// Resolves on Ctrl+C, SIGTERM, a tray "Quit", or a local control request.
///
/// The inner `select!` matters as much as the outer one: a rebind must not look
/// like a stop, so the two outcomes are reported separately rather than both
/// falling through to the same arm.
async fn shutdown_signal(shutdown: Arc<Notify>, rebind: Arc<Notify>) -> bool {
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

    enum Triggered {
        Stop,
        Rebind,
    }

    let triggered = tokio::select! {
        _ = ctrl_c => Triggered::Stop,
        _ = terminate => Triggered::Stop,
        _ = shutdown.notified() => Triggered::Stop,
        _ = rebind.notified() => Triggered::Rebind,
    };

    match triggered {
        Triggered::Stop => {
            tracing::info!("shutdown signal received");
            true
        }
        Triggered::Rebind => false,
    }
}
