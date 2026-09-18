//! End-to-end guard for the background-process contract.
//!
//! These tests drive the real binary, because what `serve`/`stop` promise is a
//! property of the process and the files on disk — unit tests over the parser
//! cannot see any of it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long to wait for the router to start answering.
const STARTUP: Duration = Duration::from_secs(30);

/// Runs the binary under test inside its own router home.
fn router(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_alnair-router"));
    command.env("ALNAIR_ROUTER_HOME", home);
    command
}

/// A child that is killed and reaped when the test ends.
struct Guard(Child);

impl Guard {
    /// Spawns `serve --foreground` against this home.
    fn serve(home: &Path) -> Self {
        let child = router(home)
            .args(["serve", "--foreground"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn serve");
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    fn exited(&mut self) -> bool {
        self.0.try_wait().expect("try_wait").is_some()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A port nothing is using, released again before the child binds it.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("addr").port()
}

/// A port plus the address the child will serve on.
fn address() -> (u16, String) {
    let port = free_port();
    (port, format!("127.0.0.1:{port}"))
}

/// A free port different from `other`, released again before it is bound.
fn other_free_port(other: u16) -> u16 {
    loop {
        let port = free_port();
        if port != other {
            return port;
        }
    }
}

/// Writes a config with a known secrets key on the given port.
fn write_config(home: &Path, port: u16) {
    std::fs::create_dir_all(home).expect("home");
    let config = format!(
        "[server]\nhost = \"127.0.0.1\"\nport = {port}\nserve_dashboard = false\ntray = false\n\n\
         [secrets]\nkey = \"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\"\n"
    );
    std::fs::write(home.join("config.toml"), config).expect("config");
}

/// Sends a minimal HTTP/1.1 request and returns the status code.
fn http_status(
    address: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> std::io::Result<u16> {
    let stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

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
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad status line"))
}

/// Polls the health probe until it answers.
fn wait_until_ready(address: &str) -> bool {
    let deadline = Instant::now() + STARTUP;
    while Instant::now() < deadline {
        if http_status(address, "GET", "/api/health", &[]).is_ok_and(|status| status == 200) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Polls until the health probe stops answering.
fn wait_until_stopped(address: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if http_status(address, "GET", "/api/health", &[]).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Reads the control token the serving process generated.
fn control_token(home: &Path) -> String {
    let path = home.join("control.token");
    let deadline = Instant::now() + STARTUP;
    while Instant::now() < deadline {
        if let Ok(token) = std::fs::read_to_string(&path) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return token;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("control.token was never written at {}", path.display());
}

/// The PID recorded on the first line of the pid file.
fn pid_file(home: &Path) -> Option<u32> {
    std::fs::read_to_string(home.join("router.pid"))
        .ok()?
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Writes a pid file as a serving process would: a PID, then its address.
fn write_pid_file(home: &Path, pid: u32, address: &str) {
    std::fs::write(home.join("router.pid"), format!("{pid}\n{address}\n")).expect("write pid file");
}

/// Starts a foreground instance and waits for it to answer.
fn started(home: &Path, address: &str) -> Guard {
    let guard = Guard::serve(home);
    assert!(
        wait_until_ready(address),
        "the server never became ready on {address}"
    );
    guard
}

#[test]
fn foreground_serve_records_its_pid_and_a_control_token() {
    let home = tempfile::tempdir().expect("tempdir");
    let (port, address) = address();
    write_config(home.path(), port);

    let guard = started(home.path(), &address);

    assert_eq!(
        pid_file(home.path()),
        Some(guard.pid()),
        "the serving process records its own pid"
    );
    assert!(
        !control_token(home.path()).is_empty(),
        "a control token is generated for the CLI"
    );
}

#[test]
fn the_control_route_refuses_anything_without_the_local_token() {
    let home = tempfile::tempdir().expect("tempdir");
    let (port, address) = address();
    write_config(home.path(), port);

    let _guard = started(home.path(), &address);
    let token = control_token(home.path());

    for headers in [
        vec![],
        vec![("x-alnair-control", "not-the-token")],
        vec![("x-alnair-control", "")],
        // The admin API is open on loopback in this configuration; that must
        // not extend to process control.
        vec![("authorization", "Bearer anything")],
    ] {
        assert_eq!(
            http_status(&address, "POST", "/api/admin/control/shutdown", &headers)
                .expect("request"),
            401,
            "these headers must not authorize a shutdown: {headers:?}"
        );
    }
    assert!(
        http_status(&address, "GET", "/api/health", &[]).is_ok(),
        "a refused request must leave the server running"
    );

    // The real token is accepted, and the listener closes behind it.
    assert_eq!(
        http_status(
            &address,
            "POST",
            "/api/admin/control/shutdown",
            &[("x-alnair-control", token.as_str())]
        )
        .expect("request"),
        202
    );
    assert!(
        wait_until_stopped(&address, Duration::from_secs(10)),
        "the listener should close after an accepted shutdown"
    );
}

#[test]
fn stop_shuts_down_a_foreground_instance_and_clears_the_pid() {
    let home = tempfile::tempdir().expect("tempdir");
    let (port, address) = address();
    write_config(home.path(), port);

    let mut guard = started(home.path(), &address);

    let output = router(home.path()).arg("stop").output().expect("run stop");
    assert!(
        output.status.success(),
        "stop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // `stop` reached the instance this test spawned, not some other process.
    assert!(guard.exited(), "the serving process should have exited");
    assert!(
        pid_file(home.path()).is_none(),
        "the pid file is removed on the way out"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("stopped"),
        "stop should report success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn stop_is_idempotent_when_nothing_is_running() {
    let home = tempfile::tempdir().expect("tempdir");
    let (port, _address) = address();
    write_config(home.path(), port);

    let output = router(home.path()).arg("stop").output().expect("run stop");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "stop should not fail: {stdout}");
    assert!(
        stdout.contains("not running"),
        "stop should say it is not running: {stdout}"
    );
}

#[test]
fn status_reports_that_nothing_is_running_and_names_the_log() {
    let home = tempfile::tempdir().expect("tempdir");
    let (port, _address) = address();
    write_config(home.path(), port);

    let output = router(home.path())
        .arg("status")
        .output()
        .expect("run status");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "status should succeed: {stdout}");
    assert!(
        stdout.contains("not running"),
        "status should say it is not running: {stdout}"
    );
    assert!(
        stdout.contains("router.log"),
        "status should name the log file: {stdout}"
    );
}

#[test]
fn status_reports_a_running_instance() {
    let home = tempfile::tempdir().expect("tempdir");
    let (port, address) = address();
    write_config(home.path(), port);

    let guard = started(home.path(), &address);

    let output = router(home.path())
        .arg("status")
        .output()
        .expect("run status");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "status should succeed: {stdout}");
    assert!(
        stdout.contains("running") && stdout.contains(&guard.pid().to_string()),
        "status should name the running pid: {stdout}"
    );
    assert!(
        stdout.contains(&format!("127.0.0.1:{port}")),
        "status should report the listening url: {stdout}"
    );
}

#[test]
fn a_stale_pid_file_is_cleaned_up_instead_of_reported_as_running() {
    let home = tempfile::tempdir().expect("tempdir");
    let (port, address) = address();
    write_config(home.path(), port);

    // A PID that can never exist, so the file cannot fake a live instance.
    write_pid_file(home.path(), 4_294_967_290, &address);

    let output = router(home.path())
        .arg("status")
        .output()
        .expect("run status");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "status should succeed: {stdout}");
    assert!(stdout.contains("not running"), "unexpected: {stdout}");
    assert!(
        pid_file(home.path()).is_none(),
        "the stale pid file should have been removed"
    );
}

#[test]
fn conflicting_serve_flags_are_rejected_before_anything_starts() {
    let home = tempfile::tempdir().expect("tempdir");
    write_config(home.path(), free_port());

    let output = router(home.path())
        .args(["serve", "--foreground", "--detach"])
        .output()
        .expect("run serve");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "conflicting flags must fail");
    assert!(
        stderr.contains("conflicting"),
        "the error should name the conflict: {stderr}"
    );
}

/// A `--port` override must reach the detached child, and `stop`/`status` must
/// find the router even though the config never learned about that port.
#[test]
fn a_custom_port_is_used_recorded_and_stoppable() {
    let home = tempfile::tempdir().expect("tempdir");
    let (configured_port, configured_address) = address();
    let custom_port = other_free_port(configured_port);
    let custom_address = format!("127.0.0.1:{custom_port}");
    write_config(home.path(), configured_port);
    let _cleanup = DetachedGuard(home.path().to_path_buf());

    let output = router(home.path())
        .args(["serve", "--detach", "--port", &custom_port.to_string()])
        .output()
        .expect("run serve --detach --port");
    assert!(
        output.status.success(),
        "serve failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        wait_until_ready(&custom_address),
        "the router should serve on the port from the flag"
    );
    assert!(
        http_status(&configured_address, "GET", "/api/health", &[]).is_err(),
        "the port from the config should be left alone"
    );

    // The port travels in the pid file, so these need no --port of their own.
    let status = router(home.path())
        .arg("status")
        .output()
        .expect("run status");
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(
        stdout.contains(&custom_address),
        "status should report the address in use: {stdout}"
    );

    let stop = router(home.path()).arg("stop").output().expect("run stop");
    assert!(
        stop.status.success(),
        "stop failed: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(
        wait_until_stopped(&custom_address, Duration::from_secs(10)),
        "the custom-port router should have stopped"
    );
}

/// Detaching must not leave the caller's stdout open.
///
/// A shell marks its own output inheritable so its children can write there;
/// that handle reaches the CLI as a plain inherited handle. If the background
/// process keeps a copy, a pipeline such as `alnair-router serve | more` never
/// sees end-of-input and the shell appears to hang after the CLI returned.
#[test]
fn detaching_does_not_hold_the_callers_stdout_open() {
    let home = tempfile::tempdir().expect("tempdir");
    let (port, address) = address();
    write_config(home.path(), port);
    let _cleanup = DetachedGuard(home.path().to_path_buf());

    // Go through `cmd`, because that is what a shell does here.
    let mut child = Command::new("cmd")
        .args([
            "/c",
            env!("CARGO_BIN_EXE_alnair-router"),
            "serve",
            "--detach",
        ])
        .env("ALNAIR_ROUTER_HOME", home.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve --detach");

    // Read the CLI's output to end-of-input on a worker thread, so a leaked
    // handle shows up as a timeout rather than a hung test run.
    let mut stdout = child.stdout.take().expect("stdout");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let result = stdout.read_to_string(&mut text);
        let _ = sender.send(result);
    });

    // Drain stderr too, so a full pipe cannot block the CLI.
    let mut stderr = child.stderr.take().expect("stderr");
    std::thread::spawn(move || {
        let mut sink = String::new();
        let _ = stderr.read_to_string(&mut sink);
    });

    match receiver.recv_timeout(STARTUP) {
        Ok(result) => result.expect("read the CLI output"),
        Err(_) => panic!("the CLI's stdout never reached end-of-input"),
    };

    // Reap `cmd`; the pipe closing above means the CLI behind it is done too.
    child.wait().expect("reap cmd");

    // The CLI returning must not have stopped the router: it is in the
    // background, which is the whole point.
    assert!(
        wait_until_ready(&address),
        "the detached router should be serving"
    );
}

/// Stops whatever the test started, however the test ends.
struct DetachedGuard(std::path::PathBuf);

impl Drop for DetachedGuard {
    fn drop(&mut self) {
        let _ = router(&self.0)
            .args(["stop", "--force"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
