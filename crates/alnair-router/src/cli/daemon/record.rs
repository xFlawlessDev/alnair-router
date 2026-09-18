//! The PID file: what a serving process records so the CLI can find it again.
//!
//! Two lines — the PID, then the `host:port` it serves on. The address matters
//! as much as the PID: it is how `stop`/`status` reach a router whose port came
//! from a `--port` flag the config never saw.

use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// What the serving process recorded about itself.
pub(super) struct Record {
    pub pid: u32,
    /// `host:port` the control request is sent to.
    pub address: String,
}

/// A PID file that removes itself when the serving process exits.
pub struct PidFile;

impl PidFile {
    /// Records the current process and the address it serves on.
    ///
    /// The address must be the one in use rather than whatever the config says
    /// today, because that is what the CLI will try to reach.
    pub fn write(address: &str) -> Result<Self> {
        let path = super::pid_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }

        // Written whole and renamed into place: a reader must never catch half
        // a record, which would look like a stale file and be deleted.
        let temporary = path.with_extension("pid.tmp");
        std::fs::write(&temporary, format!("{}\n{}\n", std::process::id(), address))
            .map_err(io_error)?;
        std::fs::rename(&temporary, &path).map_err(io_error)?;

        Ok(Self)
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        remove_pid_file();
    }
}

/// Removes the PID file, ignoring a missing one.
pub(super) fn remove_pid_file() {
    let _ = std::fs::remove_file(super::pid_path());
}

/// Reads the record, requiring both a PID and an address.
///
/// A half-written or older file is treated as absent rather than guessed at:
/// the address is the only way back to the process.
pub(super) fn read_record() -> Option<Record> {
    parse_record(&std::fs::read_to_string(super::pid_path()).ok()?)
}

/// Parses the two-line PID file: the PID, then the address served on.
fn parse_record(contents: &str) -> Option<Record> {
    let mut lines = contents.lines().map(str::trim);

    let pid = lines.next()?.parse().ok()?;
    let address = lines.next()?;
    if address.is_empty() {
        return None;
    }

    Some(Record {
        pid,
        address: address.to_string(),
    })
}

/// The record of a process that is still alive, if any.
///
/// Liveness is the PID, not the health probe: `start` and `restart` must see a
/// router that is hung rather than conclude nothing is running and collide with
/// it on the port.
pub(super) fn live_process() -> Option<Record> {
    let record = read_record()?;
    if !pid_is_alive(record.pid) {
        remove_pid_file();
        return None;
    }
    Some(record)
}

/// True while a process with this PID exists and can still serve.
///
/// On Unix a process that has exited but not been reaped yet stays in the
/// process table as a zombie; a router that already stopped must not read as
/// alive, or `stop` waits out its timeouts and tells the operator to retry with
/// `--force`.
///
/// Linux is answered from `/proc` rather than `kill -0`: the helper process
/// that `kill` needs can itself fail to fork under load, which would report a
/// running router as gone and make `stop` return before the process exited.
pub(super) fn pid_is_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .is_ok_and(|stat| !stat_reports_zombie(&stat))
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        signal_exists(pid)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let queried = unsafe { GetExitCodeProcess(handle, &mut code) };
        unsafe { CloseHandle(handle) };

        queried != 0 && code == STILL_ACTIVE as u32
    }
}

/// `kill -0` only tests for the process' existence.
///
/// Other Unix systems keep zombies too, but without `/proc` the only cheap
/// signal is `kill -0`. The case is rare in practice: a detached router is
/// reaped by init, not left to linger.
#[cfg(all(unix, not(target_os = "linux")))]
fn signal_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Reads the state field of a `/proc/<pid>/stat` line, where `Z` is a zombie.
///
/// The second field is the command name in parentheses and may contain spaces,
/// so the state is the field after the *last* `)`.
#[cfg(target_os = "linux")]
fn stat_reports_zombie(stat: &str) -> bool {
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .is_some_and(|state| state == "Z")
}

/// Other Unix systems keep zombies too, but without `/proc` the only cheap
/// signal is `kill -0`. The case is rare in practice: a detached router is
/// reaped by init, not left to linger.
#[cfg(all(unix, not(target_os = "linux")))]
fn is_zombie(_pid: u32) -> bool {
    false
}

/// Blocks until the PID disappears, up to `timeout`.
pub(super) fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_is_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    !pid_is_alive(pid)
}

fn io_error(error: std::io::Error) -> Error {
    Error::Config(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_pid_and_address_record() {
        let record = parse_record("1234\n127.0.0.1:9000\n").expect("record");

        assert_eq!(record.pid, 1234);
        assert_eq!(record.address, "127.0.0.1:9000");
    }

    #[test]
    fn a_record_without_an_address_is_not_usable() {
        // The address is the only route back to the process, so a file that
        // lacks one is treated as absent rather than guessed at.
        for contents in ["", "1234", "1234\n", "not-a-pid\n127.0.0.1:9000\n"] {
            assert!(parse_record(contents).is_none(), "contents: {contents:?}");
        }
    }

    #[test]
    fn this_process_is_alive_and_an_impossible_pid_is_not() {
        assert!(pid_is_alive(std::process::id()));
        assert!(!pid_is_alive(4_294_967_290));
    }

    /// The fields are `pid (comm) state ...`; `comm` is unquoted, so a name
    /// containing a bracket must not shift the state field.
    #[cfg(target_os = "linux")]
    #[test]
    fn zombie_state_is_read_from_the_stat_line() {
        assert!(stat_reports_zombie(
            "42 (alnair-router) Z 1 42 42 0 -1 4194560"
        ));
        assert!(!stat_reports_zombie(
            "42 (alnair-router) S 1 42 42 0 -1 4194560"
        ));
        assert!(!stat_reports_zombie("42 (weird (name) R 1 42 42 0 -1"));
    }
}
