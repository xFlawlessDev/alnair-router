//! Background process control: detaching, logging, and start/stop/restart.
//!
//! `serve` runs on the calling thread today, which keeps a terminal open for as
//! long as the router lives. When that terminal is interactive the CLI instead
//! re-executes itself detached, logging to `$ALNAIR_ROUTER_HOME/logs/router.log`,
//! and records where it serves so `stop` can shut it down gracefully.
//!
//! Split by concern: [`record`] owns the PID file, [`control`] the local
//! credential and stop request, and this module the process lifecycle.

mod control;
mod record;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::cli::ServeMode;
use crate::error::{Error, Result};

pub use control::{CONTROL_HEADER, ensure_control_token, read_control_token};
pub use record::PidFile;

use control::{health_ok, request_shutdown, wait_for_health};
use record::{live_process, pid_is_alive, read_record, remove_pid_file, wait_for_exit};

/// Log file name inside the router home.
const LOG_FILE: &str = "logs/router.log";
/// PID file name inside the router home.
const PID_FILE: &str = "router.pid";
/// Log files larger than this are rotated to `router.log.1` before a start.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
/// How long `start` waits for the new process to answer `/api/health`.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

/// Path of the background log file.
pub fn log_path() -> PathBuf {
    crate::config::router_home().join(LOG_FILE)
}

/// Path of the PID file.
pub fn pid_path() -> PathBuf {
    crate::config::router_home().join(PID_FILE)
}

/// True when the environment asks for the old foreground behaviour.
pub fn foreground_requested() -> bool {
    std::env::var("ALNAIR_ROUTER_FOREGROUND").is_ok_and(|value| is_truthy(&value))
}

/// Truthy spellings for the foreground environment flag.
fn is_truthy(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.eq_ignore_ascii_case("0")
        && !value.eq_ignore_ascii_case("false")
        && !value.eq_ignore_ascii_case("no")
}

/// Decides whether `serve` should hand off to a detached child.
///
/// `console_attached` means "launched from an interactive terminal": stdout is a
/// TTY on Unix, or the Windows parent console was attached. With no terminal
/// there is nothing to detach from — that is the auto-start, double-click and
/// container case, which keeps serving in-process. PID 1 is always a container
/// init and must stay in the foreground to keep the container alive.
pub fn should_detach(mode: ServeMode, console_attached: bool) -> bool {
    if mode == ServeMode::Foreground {
        return false;
    }

    #[cfg(unix)]
    if std::process::id() == 1 {
        if mode == ServeMode::Detach {
            tracing::warn!("running as PID 1 (container init); serving in the foreground");
        }
        return false;
    }

    match mode {
        ServeMode::Foreground => false,
        ServeMode::Detach => true,
        ServeMode::Auto => console_attached,
    }
}

/// Starts the router in the background, or reports the running instance.
pub fn start(tray: Option<bool>, port: Option<u16>) -> Result<()> {
    let config = load_config(port)?;
    let address = config.server.browser_address();

    if let Some(record) = live_process() {
        println!(
            "alnair-router is already running (pid {}) at http://{}",
            record.pid, record.address
        );
        if !health_ok(&record.address) {
            println!("  it is not answering /api/health; `alnair-router stop --force` clears it");
        }
        return Ok(());
    }

    let home = prepare_home()?;
    let log = open_log()?;
    let exe = std::env::current_exe()
        .map_err(|error| Error::Config(format!("cannot resolve the executable path: {error}")))?;

    let mut command = Command::new(&exe);
    command.arg("serve").arg("--foreground");
    match tray {
        Some(true) => {
            command.arg("--tray");
        }
        Some(false) => {
            command.arg("--no-tray");
        }
        None => {}
    }
    // The port has to travel with the child: the CLI's own override is not in
    // the config file the child reads.
    if let Some(port) = port {
        command.arg("--port").arg(port.to_string());
    }

    // The child must not hold the terminal: no stdin, and both output streams
    // land in the log file so `init_tracing` keeps working unchanged.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().map_err(io_error)?))
        .stderr(Stdio::from(log));
    // An absolute home plus a matching working directory keeps a relative
    // `storage.url` meaning the same thing once the original terminal is gone.
    command.current_dir(&home).env("ALNAIR_ROUTER_HOME", &home);

    detach(&mut command);

    let child = command
        .spawn()
        .map_err(|error| Error::Config(format!("cannot start alnair-router: {error}")))?;
    let pid = child.id();

    if wait_for_health(&address, STARTUP_TIMEOUT) {
        println!(
            "alnair-router started in the background (pid {pid}) at {}",
            config.server.browser_url()
        );
        println!("  log:  {}", log_path().display());
        println!("  stop: alnair-router stop");
        Ok(())
    } else {
        Err(Error::Config(format!(
            "alnair-router did not become ready; check {}",
            log_path().display()
        )))
    }
}

/// Stops the background router, escalating from control request to kill.
///
/// Only the PID has to be alive here: a router that stopped answering must
/// still be stoppable, which is what the `--force` escalation is for.
pub fn stop(force: bool) -> Result<()> {
    // The port comes from the running process, not from the config: it may have
    // been chosen by a `--port` flag that the config never saw.
    let Some(record) = read_record() else {
        println!("alnair-router is not running");
        return Ok(());
    };
    let pid = record.pid;

    if !pid_is_alive(pid) {
        remove_pid_file();
        println!("alnair-router is not running (removed a stale pid file)");
        return Ok(());
    }

    // The control request is the graceful path; a signal is the fallback, since
    // `shutdown_signal` already turns SIGTERM into the same graceful stop.
    if request_shutdown(&record.address).is_ok() && wait_for_exit(pid, Duration::from_secs(10)) {
        println!("alnair-router stopped (pid {pid})");
        return Ok(());
    }
    if terminate(pid) && wait_for_exit(pid, Duration::from_secs(5)) {
        println!("alnair-router stopped (pid {pid})");
        return Ok(());
    }

    if !force {
        return Err(Error::Config(format!(
            "alnair-router (pid {pid}) did not stop gracefully; retry with `alnair-router stop --force`"
        )));
    }

    kill(pid)?;
    if wait_for_exit(pid, Duration::from_secs(5)) {
        println!("alnair-router killed (pid {pid})");
        return Ok(());
    }

    Err(Error::Config(format!(
        "cannot stop alnair-router (pid {pid})"
    )))
}

/// Stops the running router (if any) and starts it again.
pub fn restart(force: bool, port: Option<u16>) -> Result<()> {
    if live_process().is_some() {
        stop(force)?;
    }
    start(None, port)
}

/// Prints whether a router process is running and where its files live.
pub(crate) fn print_process_status() -> Result<()> {
    match read_record() {
        Some(record) if pid_is_alive(record.pid) => {
            println!("process:    running (pid {})", record.pid);
            println!("url:        http://{}", record.address);
            if !health_ok(&record.address) {
                println!("health:     not answering /api/health");
            }
        }
        Some(_) => {
            remove_pid_file();
            println!("process:    not running (removed a stale pid file)");
        }
        None => println!("process:    not running"),
    }
    println!("log:        {}", log_path().display());

    Ok(())
}

/// Loads the config with this run's port override applied.
///
/// The override only touches the in-memory config: `server.port` stays a
/// deployment value, and nothing is written back to `config.toml`.
pub fn load_config(port: Option<u16>) -> Result<crate::config::RouterConfig> {
    let config = crate::config::load()?;
    Ok(with_port(config, port))
}

/// Applies this run's port override to an already-loaded config.
fn with_port(
    mut config: crate::config::RouterConfig,
    port: Option<u16>,
) -> crate::config::RouterConfig {
    if let Some(port) = port {
        config.server.port = port;
    }
    config
}

/// Makes the child survive its parent terminal.
fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // A new process group takes the child out of the terminal's foreground
        // group, so closing the terminal cannot deliver SIGHUP to it.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // DETACHED_PROCESS: no inherited console, so the child keeps running
        // after the terminal closes and the shell returns its prompt.
        // CREATE_NO_WINDOW: never allocate one in its place.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
        stop_inheriting_handles();
    }
}

/// Stops the child from inheriting handles it was never given.
///
/// `CreateProcessW` copies every handle marked inheritable, and a shell marks
/// its own output inheritable so its children can write there. That handle is
/// not necessarily one of our std handles — a GUI-subsystem binary launched
/// from a pipeline gets the pipe as a plain inherited handle — so the whole
/// table is swept rather than just `STD_*`. Without this a background router
/// would hold a copy of the caller's stdout and `alnair-router serve | more`
/// would never see end-of-input, leaving the shell hung after the CLI returned.
///
/// Only the inheritance bit is cleared: the handles stay open and usable here,
/// and the stdio the child is actually given is re-marked at spawn time.
#[cfg(windows)]
fn stop_inheriting_handles() {
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };

    // Handle values are 4-byte aligned and stay well below this for a CLI.
    for value in (4..0x1_0000).step_by(4) {
        let handle = value as *mut core::ffi::c_void;
        let mut flags = 0u32;
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
            continue;
        }
        if flags & HANDLE_FLAG_INHERIT != 0 {
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

/// Creates the router home, resolving it to an absolute path.
fn prepare_home() -> Result<PathBuf> {
    let home = crate::config::router_home();
    std::fs::create_dir_all(&home).map_err(io_error)?;

    Ok(std::fs::canonicalize(&home).unwrap_or(home))
}

/// Opens the log file for appending, rotating it once it grows too large.
fn open_log() -> Result<std::fs::File> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }

    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(io_error)
}

/// Unix: SIGTERM, which `shutdown_signal` already turns into a graceful stop.
#[cfg(unix)]
fn terminate(pid: u32) -> bool {
    signal(pid, "-TERM")
}

/// Windows has no catchable SIGTERM: anything past the control request needs
/// `stop --force`.
#[cfg(not(unix))]
fn terminate(_pid: u32) -> bool {
    false
}

/// Unix: SIGKILL, for the `--force` path.
#[cfg(unix)]
fn kill(pid: u32) -> Result<()> {
    if signal(pid, "-KILL") {
        Ok(())
    } else {
        Err(Error::Config(format!("cannot kill pid {pid}")))
    }
}

/// Windows: `TerminateProcess`, for the `--force` path.
#[cfg(windows)]
fn kill(pid: u32) -> Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        return Err(Error::Config(format!(
            "cannot open pid {pid} to terminate it"
        )));
    }
    let terminated = unsafe { TerminateProcess(handle, 1) };
    unsafe { CloseHandle(handle) };

    if terminated == 0 {
        return Err(Error::Config(format!("cannot terminate pid {pid}")));
    }
    Ok(())
}

/// Sends a signal with `kill`, reporting whether it was delivered.
#[cfg(unix)]
fn signal(pid: u32, name: &str) -> bool {
    Command::new("kill")
        .args([name, &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn io_error(error: std::io::Error) -> Error {
    Error::Config(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_never_detaches() {
        assert!(!should_detach(ServeMode::Foreground, true));
        assert!(!should_detach(ServeMode::Foreground, false));
    }

    #[test]
    fn explicit_detach_detaches_even_without_a_console() {
        // Only Unix short-circuits on PID 1; the test runner is never PID 1.
        assert!(should_detach(ServeMode::Detach, false));
    }

    #[test]
    fn auto_detaches_only_from_an_interactive_console() {
        assert!(should_detach(ServeMode::Auto, true));
        assert!(
            !should_detach(ServeMode::Auto, false),
            "auto-start, double-click and containers must stay in-process"
        );
    }

    #[test]
    fn foreground_environment_flag_accepts_only_truthy_spellings() {
        for value in ["1", "true", "YES", "on"] {
            assert!(is_truthy(value), "{value} should read as foreground");
        }
        for value in ["", "  ", "0", "false", "no"] {
            assert!(!is_truthy(value), "{value} should not");
        }
    }

    #[test]
    fn paths_live_under_the_router_home() {
        let home = crate::config::router_home();
        assert_eq!(pid_path(), home.join("router.pid"));
        assert_eq!(log_path(), home.join("logs").join("router.log"));
    }

    #[test]
    fn a_port_override_replaces_the_configured_port() {
        let base = crate::config::RouterConfig::default();
        let configured = base.server.port;

        assert_eq!(with_port(base.clone(), Some(9000)).server.port, 9000);
        assert_eq!(with_port(base, None).server.port, configured);
    }
}
