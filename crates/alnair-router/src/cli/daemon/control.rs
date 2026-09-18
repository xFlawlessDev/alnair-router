//! The local control channel: the token file, the stop request, and the probe.
//!
//! `stop` and `status` have to reach a running router without depending on how
//! it was configured. `server.admin_token` may be unset and the dashboard
//! password may not exist yet, so the admin guard would accept a request from
//! any local process. The control token is a separate 32-byte secret, readable
//! only by the account that owns the router home; a browser page cannot read
//! that file, so a hostile site cannot forge the request either.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// Header carrying the local control token.
pub const CONTROL_HEADER: &str = "x-alnair-control";
/// Control token file name inside the router home.
const CONTROL_FILE: &str = "control.token";

/// Path of the local control token.
pub(super) fn control_token_path() -> PathBuf {
    crate::config::router_home().join(CONTROL_FILE)
}

/// Returns the control token, generating and persisting one when missing.
///
/// This is deliberately not `secrets.key`: the key that encrypts upstream
/// credentials must not double as a network credential.
pub fn ensure_control_token() -> Result<String> {
    ensure_token_at(&control_token_path())
}

/// Reads the existing control token, if any.
pub fn read_control_token() -> Option<String> {
    read_token_at(&control_token_path())
}

/// Creates the token file once, then reuses whatever is there.
fn ensure_token_at(path: &Path) -> Result<String> {
    if read_token_at(path).is_none() {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        write_file(path, &token)?;
    }

    read_token_at(path).ok_or_else(|| {
        Error::Config(format!(
            "cannot read the control token at {}",
            path.display()
        ))
    })
}

/// Reads a token file, treating a blank file as absent.
fn read_token_at(path: &Path) -> Option<String> {
    let token = std::fs::read_to_string(path).ok()?;
    let token = token.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Writes a file with owner-only permissions where the platform supports it.
fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    std::fs::write(path, contents).map_err(io_error)?;
    restrict_permissions(path);
    Ok(())
}

/// Best-effort owner-only permissions for generated secrets.
#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

/// Asks the running router to shut down using the local control token.
pub(super) fn request_shutdown(address: &str) -> Result<()> {
    let token = read_control_token().ok_or_else(|| {
        Error::Config(format!(
            "cannot read the control token at {}",
            control_token_path().display()
        ))
    })?;

    let status = http_status(
        address,
        "POST",
        "/api/admin/control/shutdown",
        &[(CONTROL_HEADER, token.as_str())],
    )
    .map_err(|error| Error::Config(format!("cannot reach {address}: {error}")))?;

    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "the control request was refused with {status}"
        )))
    }
}

/// Polls `/api/health` until it answers, up to `timeout`.
pub(super) fn wait_for_health(address: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if health_ok(address) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    health_ok(address)
}

/// True when the router answers its liveness probe.
pub(super) fn health_ok(address: &str) -> bool {
    http_status(address, "GET", "/api/health", &[]).is_ok_and(|status| status == 200)
}

/// Sends a minimal HTTP/1.1 request over loopback and returns the status code.
///
/// The CLI only ever reaches the router on the local machine in plain HTTP, so
/// this avoids pulling an HTTP client into the startup path.
fn http_status(
    address: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> std::io::Result<u16> {
    let stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;

    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("Content-Length: 0\r\n\r\n");

    let mut stream = stream;
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    line.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "malformed status line")
        })
}

fn io_error(error: std::io::Error) -> Error {
    Error::Config(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_control_header_name_is_stable() {
        assert_eq!(CONTROL_HEADER, "x-alnair-control");
    }

    #[test]
    fn the_control_token_is_generated_once_and_reused() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("nested").join("control.token");

        let generated = ensure_token_at(&path).expect("generate");
        assert_eq!(generated.len(), 64, "32 bytes as hex");
        assert!(generated.chars().all(|char| char.is_ascii_hexdigit()));
        assert_eq!(ensure_token_at(&path).expect("reuse"), generated);
        assert_eq!(read_token_at(&path), Some(generated));
    }

    #[test]
    fn a_blank_or_missing_token_file_reads_as_absent() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("control.token");

        assert_eq!(read_token_at(&path), None, "missing");
        std::fs::write(&path, "   \n").expect("write");
        assert_eq!(read_token_at(&path), None, "blank");
    }

    #[test]
    fn an_unreachable_port_is_reported_as_not_healthy() {
        // Nothing listens on port 1, so the probe must fail rather than hang.
        assert!(http_status("127.0.0.1:1", "GET", "/api/health", &[]).is_err());
        assert!(!health_ok("127.0.0.1:1"));
    }
}
