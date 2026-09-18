//! Command-line entrypoints: `serve` (default), `start`, `stop`, `restart`,
//! `install`, `uninstall`, `status`.

pub mod autostart;
pub mod daemon;

use crate::error::{Error, Result};

pub use autostart::{ensure_config, generated_key, install, uninstall};

/// How `serve` should be started relative to the calling terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeMode {
    /// Detach when launched from an interactive terminal, otherwise serve here.
    Auto,
    /// Serve in this process, blocking the terminal (the pre-daemon behaviour).
    Foreground,
    /// Always hand off to a detached background process.
    Detach,
}

/// Parsed command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Run the HTTP server (default). The options override `server.*` for this
    /// run only.
    Serve {
        tray: Option<bool>,
        mode: ServeMode,
        port: Option<u16>,
    },
    /// Start the router in the background.
    Start {
        tray: Option<bool>,
        port: Option<u16>,
    },
    /// Stop the background router.
    Stop {
        force: bool,
    },
    /// Stop the background router, then start it again.
    Restart {
        force: bool,
        port: Option<u16>,
    },
    /// Write a config with a generated secrets key and enable auto-start.
    Install,
    /// Disable auto-start.
    Uninstall,
    /// Report whether the router runs and whether auto-start is enabled.
    Status,
    Help,
    Version,
}

impl Command {
    /// Parses argv (the program name is skipped).
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut args = args.into_iter();
        let _program = args.next();

        let Some(flag) = args.next() else {
            return Ok(Command::Serve {
                tray: None,
                mode: ServeMode::Auto,
                port: None,
            });
        };

        match flag.as_str() {
            "serve" | "--serve" => Self::parse_serve_flags(args),
            "start" => Self::parse_start_flags(args),
            "stop" => Self::parse_stop_flags(args),
            "restart" => Self::parse_restart_flags(args),
            "install" | "--install" => no_arguments(args, Command::Install),
            "uninstall" | "--uninstall" => no_arguments(args, Command::Uninstall),
            "status" | "--status" => no_arguments(args, Command::Status),
            "-h" | "--help" | "help" => no_arguments(args, Command::Help),
            "-V" | "--version" | "version" => no_arguments(args, Command::Version),
            // Any other flag is a `serve` shorthand: `alnair-router --port 9000`
            // is `alnair-router serve --port 9000`. Unknown ones are reported by
            // the options parser.
            other if other.starts_with('-') => Self::parse_serve_flags(once(other).chain(args)),
            other => Err(unknown("command", other)),
        }
    }

    /// Parses the overrides that may follow `serve`.
    fn parse_serve_flags(args: impl Iterator<Item = String>) -> Result<Self> {
        let options = RunOptions::parse(args)?;

        Ok(Command::Serve {
            tray: options.tray,
            mode: options.mode.unwrap_or(ServeMode::Auto),
            port: options.port,
        })
    }

    /// Parses `start`, which accepts the same overrides but always detaches.
    fn parse_start_flags(args: impl Iterator<Item = String>) -> Result<Self> {
        let options = RunOptions::parse(args)?;

        Ok(Command::Start {
            tray: options.tray,
            port: options.port,
        })
    }

    /// Parses `stop`, which accepts only `--force`: it finds the running router
    /// through its pid file, so a port would mean nothing here.
    fn parse_stop_flags(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut force = false;

        for arg in args {
            match arg.as_str() {
                "--force" | "-f" => force = true,
                other => return Err(unknown("option", other)),
            }
        }

        Ok(Command::Stop { force })
    }

    /// Parses `restart`: the port applies to the start half of the pair.
    fn parse_restart_flags(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut force = false;
        let mut port = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--force" | "-f" => force = true,
                "--port" => set_once(&mut port, take_port_value(&mut args)?)?,
                other => return Err(unknown("option", other)),
            }
        }

        Ok(Command::Restart { force, port })
    }
}

/// A single value as an iterator, so shorthand flags can share the parser.
fn once(flag: &str) -> std::iter::Once<String> {
    std::iter::once(flag.to_string())
}

/// Overrides shared by `serve` and `start`.
#[derive(Default)]
struct RunOptions {
    tray: Option<bool>,
    mode: Option<ServeMode>,
    port: Option<u16>,
}

impl RunOptions {
    /// Parses the flags that may follow `serve` or `start`.
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self::default();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--tray" => options.set_tray(true)?,
                "--no-tray" => options.set_tray(false)?,
                "--foreground" => options.set_mode(ServeMode::Foreground)?,
                "--detach" => options.set_mode(ServeMode::Detach)?,
                "--port" => set_once(&mut options.port, take_port_value(&mut args)?)?,
                other => return Err(unknown("option", other)),
            }
        }

        Ok(options)
    }

    fn set_tray(&mut self, requested: bool) -> Result<()> {
        if self.tray.is_some_and(|current| current != requested) {
            return Err(Error::Config(
                "conflicting options: --tray and --no-tray".to_string(),
            ));
        }
        self.tray = Some(requested);
        Ok(())
    }

    fn set_mode(&mut self, requested: ServeMode) -> Result<()> {
        if self.mode.is_some_and(|current| current != requested) {
            return Err(Error::Config(
                "conflicting options: --foreground and --detach".to_string(),
            ));
        }
        self.mode = Some(requested);
        Ok(())
    }
}

/// Rejects a repeated option that names a different value.
fn set_once<T: PartialEq + Copy>(slot: &mut Option<T>, value: T) -> Result<()> {
    if slot.is_some_and(|current| current != value) {
        return Err(Error::Config(
            "conflicting values for the same option".to_string(),
        ));
    }
    *slot = Some(value);
    Ok(())
}

/// Consumes the value of a `--port` flag.
fn take_port_value(args: &mut impl Iterator<Item = String>) -> Result<u16> {
    let value = args
        .next()
        .ok_or_else(|| Error::Config("--port needs a value, e.g. --port 9000".to_string()))?;

    parse_port(&value)
}

/// Parses a TCP port, rejecting 0 and anything out of range.
fn parse_port(value: &str) -> Result<u16> {
    match value.trim().parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(Error::Config(format!(
            "invalid port '{value}'; expected 1-65535"
        ))),
    }
}

/// Accepts a command only when nothing follows it.
fn no_arguments(args: impl Iterator<Item = String>, command: Command) -> Result<Command> {
    match args.into_iter().next() {
        Some(arg) => Err(unknown("option", &arg)),
        None => Ok(command),
    }
}

fn unknown(kind: &str, value: &str) -> Error {
    Error::Config(format!("unknown {kind} '{value}'; try --help"))
}

/// Help text for `--help`.
pub fn help() -> String {
    format!(
        "\
alnair-router {version}

Usage:
  alnair-router [serve]        Run the HTTP server (default). Detaches into the
                               background when launched from a terminal
  alnair-router start          Start the router in the background
  alnair-router stop           Stop the background router gracefully
  alnair-router restart        Stop it, then start it again
  alnair-router status         Show process, auto-start state and paths
  alnair-router install        Create {home}/config.toml with a generated
                               secrets key and register auto-start
  alnair-router uninstall      Disable auto-start (keeps data)
  alnair-router --help         Show this help
  alnair-router --version      Print the version

Options:
  --port <PORT>                Listen on this port for this run instead of
                               server.port (1-65535)
  --foreground                 Serve in this terminal instead of detaching
                               (equivalent to ALNAIR_ROUTER_FOREGROUND=1)
  --detach                     Always hand off to a background process
  --force                      With stop/restart: kill if a graceful stop fails
  --tray / --no-tray           Force the system tray icon on or off while
                               serving (default: server.tray; Windows and
                               macOS only)

`stop` and `status` read the port from {home}/router.pid, so they find the
router however it was started. Background runs log to {home}/logs/router.log.
",
        version = env!("CARGO_PKG_VERSION"),
        home = crate::config::router_home().display(),
    )
}

/// Prints the process state plus the auto-start state and paths.
pub fn status() -> Result<()> {
    daemon::print_process_status()?;

    let home = crate::config::router_home();
    let exe = std::env::current_exe()
        .map_err(|error| Error::Config(format!("cannot resolve the executable path: {error}")))?;
    let launcher = autostart::auto_launch(&exe)?;
    let enabled = launcher
        .is_enabled()
        .map_err(|error| Error::Config(format!("failed to read auto-start state: {error}")))?;

    println!(
        "auto-start: {}",
        if enabled { "enabled" } else { "disabled" }
    );
    println!("binary:     {}", exe.display());
    println!("config:     {}", home.join("config.toml").display());
    println!(
        "database:   {}",
        home.join("db").join("router.sqlite").display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command> {
        Command::parse(args.iter().map(|arg| arg.to_string()))
    }

    fn serve(tray: Option<bool>, mode: ServeMode) -> Command {
        Command::Serve {
            tray,
            mode,
            port: None,
        }
    }

    #[test]
    fn parses_commands() {
        assert_eq!(
            parse(&["alnair-router"]).expect("valid"),
            serve(None, ServeMode::Auto)
        );
        assert_eq!(
            parse(&["alnair-router", "serve"]).expect("valid"),
            serve(None, ServeMode::Auto)
        );
        assert_eq!(
            parse(&["alnair-router", "start"]).expect("valid"),
            Command::Start {
                tray: None,
                port: None
            }
        );
        assert_eq!(
            parse(&["alnair-router", "stop"]).expect("valid"),
            Command::Stop { force: false }
        );
        assert_eq!(
            parse(&["alnair-router", "restart"]).expect("valid"),
            Command::Restart {
                force: false,
                port: None
            }
        );
        assert_eq!(
            parse(&["alnair-router", "install"]).expect("valid"),
            Command::Install
        );
        assert_eq!(
            parse(&["alnair-router", "uninstall"]).expect("valid"),
            Command::Uninstall
        );
        assert_eq!(
            parse(&["alnair-router", "status"]).expect("valid"),
            Command::Status
        );
        assert_eq!(
            parse(&["alnair-router", "--version"]).expect("valid"),
            Command::Version
        );
        assert!(parse(&["alnair-router", "nope"]).is_err());
    }

    #[test]
    fn parses_tray_overrides() {
        assert_eq!(
            parse(&["alnair-router", "--no-tray"]).expect("valid"),
            serve(Some(false), ServeMode::Auto)
        );
        assert_eq!(
            parse(&["alnair-router", "serve", "--tray"]).expect("valid"),
            serve(Some(true), ServeMode::Auto)
        );
        assert_eq!(
            parse(&["alnair-router", "serve", "--no-tray", "--no-tray"]).expect("valid"),
            serve(Some(false), ServeMode::Auto)
        );
        assert_eq!(
            parse(&["alnair-router", "start", "--no-tray"]).expect("valid"),
            Command::Start {
                tray: Some(false),
                port: None
            }
        );
    }

    #[test]
    fn parses_detach_modes() {
        assert_eq!(
            parse(&["alnair-router", "--foreground"]).expect("valid"),
            serve(None, ServeMode::Foreground)
        );
        assert_eq!(
            parse(&["alnair-router", "--detach"]).expect("valid"),
            serve(None, ServeMode::Detach)
        );
        assert_eq!(
            parse(&["alnair-router", "serve", "--detach", "--tray"]).expect("valid"),
            serve(Some(true), ServeMode::Detach)
        );
    }

    #[test]
    fn parses_a_custom_port() {
        assert_eq!(
            parse(&["alnair-router", "--port", "9000"]).expect("valid"),
            Command::Serve {
                tray: None,
                mode: ServeMode::Auto,
                port: Some(9000)
            }
        );
        assert_eq!(
            parse(&["alnair-router", "serve", "--foreground", "--port", "65535"]).expect("valid"),
            Command::Serve {
                tray: None,
                mode: ServeMode::Foreground,
                port: Some(65535)
            }
        );
        assert_eq!(
            parse(&["alnair-router", "start", "--port", "1"]).expect("valid"),
            Command::Start {
                tray: None,
                port: Some(1)
            }
        );
        assert_eq!(
            parse(&["alnair-router", "restart", "--port", "8080"]).expect("valid"),
            Command::Restart {
                force: false,
                port: Some(8080)
            }
        );
    }

    #[test]
    fn rejects_out_of_range_or_missing_ports() {
        for value in ["0", "65536", "-1", "abc", "", "  "] {
            assert!(
                parse(&["alnair-router", "serve", "--port", value]).is_err(),
                "'{value}' is not a usable port"
            );
        }
        assert!(parse(&["alnair-router", "serve", "--port"]).is_err());
        // `stop` finds the router through its pid file, so a port is meaningless.
        assert!(parse(&["alnair-router", "stop", "--port", "9000"]).is_err());
        assert!(
            parse(&["alnair-router", "serve", "--port", "9000", "--port", "9001"]).is_err(),
            "conflicting ports"
        );
    }

    #[test]
    fn parses_force_only_for_stop_and_restart() {
        assert_eq!(
            parse(&["alnair-router", "stop", "--force"]).expect("valid"),
            Command::Stop { force: true }
        );
        assert_eq!(
            parse(&["alnair-router", "restart", "-f"]).expect("valid"),
            Command::Restart {
                force: true,
                port: None
            }
        );
        assert_eq!(
            parse(&["alnair-router", "restart", "--force", "--port", "9000"]).expect("valid"),
            Command::Restart {
                force: true,
                port: Some(9000)
            }
        );
        assert!(parse(&["alnair-router", "start", "--force"]).is_err());
        assert!(parse(&["alnair-router", "install", "--force"]).is_err());
    }

    #[test]
    fn rejects_conflicting_or_unknown_serve_options() {
        assert!(parse(&["alnair-router", "serve", "--tray", "--no-tray"]).is_err());
        assert!(parse(&["alnair-router", "serve", "--foreground", "--detach"]).is_err());
        assert!(parse(&["alnair-router", "serve", "--nope"]).is_err());
        assert!(parse(&["alnair-router", "stop", "--nope"]).is_err());
    }

    #[test]
    fn help_lists_the_process_commands() {
        let help = help();
        for command in [
            "start",
            "stop",
            "restart",
            "status",
            "--foreground",
            "--detach",
            "--force",
            "--port",
        ] {
            assert!(help.contains(command), "help should mention {command}");
        }
    }
}
