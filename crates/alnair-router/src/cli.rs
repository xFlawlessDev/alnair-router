//! Command-line entrypoints: `serve` (default), `install`, `uninstall`, `status`.

use std::path::{Path, PathBuf};

use auto_launch::{AutoLaunch, AutoLaunchBuilder};

use crate::error::{Error, Result};

const AUTO_LAUNCH_NAME: &str = "alnair-router";

/// Parsed command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Run the HTTP server (default). `tray` overrides `server.tray` when set.
    Serve {
        tray: Option<bool>,
    },
    /// Write a config with a generated secrets key and enable auto-start.
    Install,
    /// Disable auto-start.
    Uninstall,
    /// Report whether auto-start is enabled and where state lives.
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
            return Ok(Command::Serve { tray: None });
        };

        match flag.as_str() {
            "serve" | "--serve" => Self::parse_serve_flags(args),
            "--tray" => Ok(Command::Serve { tray: Some(true) }),
            "--no-tray" => Ok(Command::Serve { tray: Some(false) }),
            "install" | "--install" => Ok(Command::Install),
            "uninstall" | "--uninstall" => Ok(Command::Uninstall),
            "status" | "--status" => Ok(Command::Status),
            "-h" | "--help" | "help" => Ok(Command::Help),
            "-V" | "--version" | "version" => Ok(Command::Version),
            other => Err(Error::Config(format!(
                "unknown command '{other}'; try --help"
            ))),
        }
    }

    /// Parses the tray overrides that may follow `serve`.
    fn parse_serve_flags(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut tray = None;

        for arg in args {
            let requested = match arg.as_str() {
                "--tray" => true,
                "--no-tray" => false,
                other => {
                    return Err(Error::Config(format!(
                        "unknown option '{other}' for serve; try --help"
                    )));
                }
            };

            if tray.is_some_and(|current| current != requested) {
                return Err(Error::Config(
                    "conflicting options: --tray and --no-tray".to_string(),
                ));
            }
            tray = Some(requested);
        }

        Ok(Command::Serve { tray })
    }
}

/// Help text for `--help`.
pub fn help() -> String {
    format!(
        "\
alnair-router {version}

Usage:
  alnair-router [serve]        Run the HTTP server (default)
  alnair-router install        Create {home}/config.toml with a generated
                               secrets key and register auto-start
  alnair-router uninstall      Disable auto-start (keeps data)
  alnair-router status         Show auto-start state and paths
  alnair-router --help         Show this help
  alnair-router --version      Print the version

Options:
  --tray / --no-tray           Force the system tray icon on or off while
                               serving (default: server.tray; Windows and
                               macOS only)
",
        version = env!("CARGO_PKG_VERSION"),
        home = crate::config::router_home().display(),
    )
}

/// Prepares the router home and enables auto-start for this executable.
pub fn install() -> Result<()> {
    let home = crate::config::router_home();
    std::fs::create_dir_all(&home)
        .map_err(|error| Error::Config(format!("cannot create {}: {error}", home.display())))?;

    let config_path = ensure_config(&home)?;
    let exe = std::env::current_exe()
        .map_err(|error| Error::Config(format!("cannot resolve the executable path: {error}")))?;

    let launcher = auto_launch(&exe)?;
    launcher
        .enable()
        .map_err(|error| Error::Config(format!("failed to enable auto-start: {error}")))?;

    println!("Installed alnair-router");
    println!("  binary:  {}", exe.display());
    println!("  config:  {}", config_path.display());
    println!(
        "  database: {}",
        home.join("db").join("router.sqlite").display()
    );
    println!("Auto-start is enabled: the router starts with your session.");

    Ok(())
}

/// Disables auto-start, leaving config and data in place.
pub fn uninstall() -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|error| Error::Config(format!("cannot resolve the executable path: {error}")))?;
    let launcher = auto_launch(&exe)?;

    if launcher.is_enabled().unwrap_or(true) {
        launcher
            .disable()
            .map_err(|error| Error::Config(format!("failed to disable auto-start: {error}")))?;
        println!(
            "Auto-start disabled. Config and data under {} are untouched.",
            crate::config::router_home().display()
        );
    } else {
        println!(
            "Auto-start was not enabled. Config and data under {} are untouched.",
            crate::config::router_home().display()
        );
    }

    Ok(())
}

/// Prints the auto-start state plus where state lives.
pub fn status() -> Result<()> {
    let home = crate::config::router_home();
    let exe = std::env::current_exe()
        .map_err(|error| Error::Config(format!("cannot resolve the executable path: {error}")))?;
    let launcher = auto_launch(&exe)?;
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

fn auto_launch(exe: &Path) -> Result<AutoLaunch> {
    AutoLaunchBuilder::new()
        .set_app_name(AUTO_LAUNCH_NAME)
        .set_app_path(&exe.to_string_lossy())
        .set_use_launch_agent(true)
        .build()
        .map_err(|error| Error::Config(format!("cannot configure auto-start: {error}")))
}

/// Creates `config.toml` with a generated secrets key when missing.
///
/// An existing config is never overwritten: if it lacks a key the user has to
/// add one deliberately.
fn ensure_config(home: &Path) -> Result<PathBuf> {
    let path = home.join("config.toml");
    if path.exists() {
        let existing = std::fs::read_to_string(&path)
            .map_err(|error| Error::Config(format!("cannot read {}: {error}", path.display())))?;
        if !existing.contains("key") {
            return Err(Error::Config(format!(
                "{} exists but has no [secrets] key; add one (openssl rand -hex 32) instead of relying on the installer",
                path.display()
            )));
        }
        return Ok(path);
    }

    let key = generated_key();
    let template = format!(
        "# Generated by `alnair-router install`.\n\
         # Edit freely; `ALNAIR_ROUTER__SECTION__KEY` overrides any value.\n\n\
         [server]\n\
         host = \"127.0.0.1\"\n\
         port = 7878\n\n\
         [secrets]\n\
         # 32 bytes as 64 hex characters. Keep it safe and backed up.\n\
         key = \"{key}\"\n"
    );
    std::fs::write(&path, template)
        .map_err(|error| Error::Config(format!("cannot write {}: {error}", path.display())))?;
    Ok(path)
}

/// 32 random bytes as 64 hex characters (two v4 UUIDs).
fn generated_key() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        let parse = |args: &[&str]| {
            Command::parse(args.iter().map(|arg| arg.to_string())).expect("valid command")
        };

        assert_eq!(parse(&["alnair-router"]), Command::Serve { tray: None });
        assert_eq!(
            parse(&["alnair-router", "serve"]),
            Command::Serve { tray: None }
        );
        assert_eq!(parse(&["alnair-router", "install"]), Command::Install);
        assert_eq!(parse(&["alnair-router", "uninstall"]), Command::Uninstall);
        assert_eq!(parse(&["alnair-router", "status"]), Command::Status);
        assert_eq!(parse(&["alnair-router", "--version"]), Command::Version);
        assert!(Command::parse(["alnair-router", "nope"].map(str::to_string)).is_err());
    }

    #[test]
    fn parses_tray_overrides() {
        let parse = |args: &[&str]| {
            Command::parse(args.iter().map(|arg| arg.to_string())).expect("valid command")
        };

        assert_eq!(
            parse(&["alnair-router", "--no-tray"]),
            Command::Serve { tray: Some(false) }
        );
        assert_eq!(
            parse(&["alnair-router", "serve", "--tray"]),
            Command::Serve { tray: Some(true) }
        );
        assert_eq!(
            parse(&["alnair-router", "serve", "--no-tray", "--no-tray"]),
            Command::Serve { tray: Some(false) }
        );
    }

    #[test]
    fn rejects_conflicting_or_unknown_serve_options() {
        let parse = |args: &[&str]| Command::parse(args.iter().map(|arg| arg.to_string()));

        assert!(parse(&["alnair-router", "serve", "--tray", "--no-tray"]).is_err());
        assert!(parse(&["alnair-router", "serve", "--nope"]).is_err());
    }

    #[test]
    fn install_writes_a_config_with_a_key() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = ensure_config(home.path()).expect("config");

        let written = std::fs::read_to_string(&path).expect("read");
        assert!(written.contains("[secrets]"));
        let key = written
            .lines()
            .find_map(|line| line.trim().strip_prefix("key = "))
            .expect("key line")
            .trim_matches('"');
        assert_eq!(key.len(), 64, "32 bytes as hex");
        assert!(key.chars().all(|char| char.is_ascii_hexdigit()));
    }

    #[test]
    fn install_never_overwrites_an_existing_config() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join("config.toml");
        std::fs::write(&path, "[secrets]\nkey = \"keep-me\"\n").expect("write");

        let resolved = ensure_config(home.path()).expect("config");

        assert_eq!(resolved, path);
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .contains("keep-me")
        );
    }

    #[test]
    fn install_refuses_a_config_without_a_key() {
        let home = tempfile::tempdir().expect("tempdir");
        std::fs::write(home.path().join("config.toml"), "[server]\nport = 7878\n").expect("write");

        assert!(ensure_config(home.path()).is_err());
    }
}
