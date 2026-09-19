//! Auto-start registration and the config bootstrap used by `install`.

use std::path::{Path, PathBuf};

use auto_launch::{AutoLaunch, AutoLaunchBuilder};

use crate::error::{Error, Result};

const AUTO_LAUNCH_NAME: &str = "alnair-router";

/// What the platform's auto-start registration says about this binary.
///
/// `is_enabled` only answers "is there a registration under this name", which
/// hides the case that matters: the entry exists but launches some *other*
/// binary — an older install, or a build that has since moved. The router then
/// looks installed while nothing comes back after a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registration {
    /// Registered, and pointing at the binary that is running.
    Current,
    /// Registered, but pointing at a different executable.
    Other(String),
    /// No registration for this name.
    Absent,
    /// The registration could not be read.
    Unknown,
}

/// Reads the auto-start registration and compares it with `exe`.
pub fn registration(exe: &Path) -> Registration {
    let launcher = match auto_launch(exe) {
        Ok(launcher) => launcher,
        Err(error) => {
            tracing::warn!(%error, "cannot build the auto-start registration handle");
            return Registration::Unknown;
        }
    };

    let enabled = match launcher.is_enabled() {
        Ok(enabled) => enabled,
        Err(error) => {
            tracing::warn!(%error, "cannot read auto-start state");
            return Registration::Unknown;
        }
    };
    if !enabled {
        return Registration::Absent;
    }

    // The registration exists. Only Windows can be asked *what* it launches;
    // elsewhere the mechanism is file-based and `is_enabled` is all we get.
    match registered_command() {
        Some(command) if same_executable(&command, exe) => Registration::Current,
        Some(command) => Registration::Other(registered_executable(&command)),
        None => Registration::Current,
    }
}

/// The executable a registered command line starts with.
///
/// The registration is written as the bare path followed by any arguments, so
/// a quoted path is read up to the closing quote and an unquoted one is taken
/// whole — the router registers no arguments, so the rest is the path.
fn registered_executable(command: &str) -> String {
    let trimmed = command.trim();
    if let Some(rest) = trimmed.strip_prefix('"')
        && let Some(end) = rest.find('"')
    {
        return rest[..end].to_string();
    }

    trimmed.to_string()
}

/// True when `command` launches `exe`.
fn same_executable(command: &str, exe: &Path) -> bool {
    let registered = registered_executable(command);

    // Windows paths are case-insensitive, and the same file can be reached
    // through different spellings, so a plain string compare would report a
    // false mismatch. Canonicalizing both settles it when the target exists;
    // the comparison falls back to the written form when it does not.
    let registered_path = Path::new(&registered);
    if let (Ok(left), Ok(right)) = (
        std::fs::canonicalize(registered_path),
        std::fs::canonicalize(exe),
    ) {
        return left == right;
    }

    registered.eq_ignore_ascii_case(&exe.to_string_lossy())
}

/// The command line the platform would launch at startup, when readable.
///
/// Windows only: the Run key stores the command verbatim. Linux (a `.desktop`
/// file) and macOS (a LaunchAgent plist) would each need their own parser, and
/// until one exists `registration` treats an enabled entry as current rather
/// than guessing at a mismatch.
#[cfg(windows)]
fn registered_command() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .ok()?
        .get_value::<String, _>(AUTO_LAUNCH_NAME)
        .ok()
}

#[cfg(not(windows))]
fn registered_command() -> Option<String> {
    None
}

/// Removes the auto-start bookkeeping `disable` leaves behind.
///
/// On Windows the Task Manager keeps its own record of whether a Run entry is
/// allowed to start. `auto-launch` writes that record on `enable` but never
/// clears it on `disable`, so uninstalling left a phantom row behind. Returns
/// whether something was actually removed.
#[cfg(windows)]
fn cleanup_registration() -> bool {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(STARTUP_APPROVED_KEY, KEY_SET_VALUE)
    else {
        return false;
    };

    key.delete_value(AUTO_LAUNCH_NAME).is_ok()
}

/// Linux and macOS remove the whole file, so nothing is left over to clean.
#[cfg(not(windows))]
fn cleanup_registration() -> bool {
    false
}

/// Run-key location the registration is written to.
#[cfg(windows)]
const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
/// Task Manager's per-entry startup override for the Run key.
#[cfg(windows)]
const STARTUP_APPROVED_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Enables auto-start for this executable.
pub(crate) fn auto_launch(exe: &Path) -> Result<AutoLaunch> {
    AutoLaunchBuilder::new()
        .set_app_name(AUTO_LAUNCH_NAME)
        .set_app_path(&exe.to_string_lossy())
        .set_use_launch_agent(true)
        .build()
        .map_err(|error| Error::Config(format!("cannot configure auto-start: {error}")))
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

    // Remove the Task Manager override as well: `disable` only clears the
    // registration itself, so the stale override would survive and leave a
    // phantom entry behind in Task Manager -> Startup.
    let cleaned = cleanup_registration();

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

    if cleaned {
        println!("Cleared a leftover startup override from Task Manager.");
    }

    Ok(())
}

/// Creates `config.toml` with a generated secrets key when missing.
///
/// An existing config is never overwritten: if it lacks a key the user has to
/// add one deliberately.
pub fn ensure_config(home: &Path) -> Result<PathBuf> {
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
pub fn generated_key() -> String {
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

    #[test]
    fn an_unquoted_registration_is_read_whole() {
        assert_eq!(
            registered_executable(r"C:\Program Files\alnair-router\alnair-router.exe"),
            r"C:\Program Files\alnair-router\alnair-router.exe"
        );
    }

    #[test]
    fn a_quoted_registration_stops_at_the_closing_quote() {
        assert_eq!(
            registered_executable(r#""C:\Program Files\alnair-router\alnair-router.exe""#),
            r"C:\Program Files\alnair-router\alnair-router.exe"
        );
        assert_eq!(
            registered_executable(r#""C:\Program Files\alnair-router\alnair-router.exe" --tray"#),
            r"C:\Program Files\alnair-router\alnair-router.exe"
        );
    }

    #[test]
    fn a_path_with_spaces_is_not_split_on_the_space() {
        // The reason the path is parsed rather than the command: splitting on
        // whitespace would truncate at "Program".
        let parsed = registered_executable(r#""C:\Program Files\App\app.exe" -x"#);
        assert!(parsed.contains("Program Files"), "got {parsed}");
    }

    #[test]
    fn matching_ignores_case_and_surrounding_whitespace() {
        let exe = Path::new(r"C:\Tools\alnair-router.exe");

        assert!(same_executable(r"C:\Tools\alnair-router.exe", exe));
        assert!(same_executable(r"c:\tools\ALNAIR-ROUTER.EXE", exe));
        assert!(same_executable(
            r#"  "C:\Tools\alnair-router.exe" --port 9000  "#,
            exe
        ));
        assert!(!same_executable(r"C:\Other\alnair-router.exe", exe));
    }

    #[test]
    fn a_registration_for_another_binary_does_not_match() {
        // The case `status` exists to catch: the entry is present, so
        // `is_enabled` says yes, but it starts a binary that is not this one.
        let exe = Path::new(r"C:\Tools\alnair-router.exe");

        assert!(!same_executable(r"C:\Old\alnair-router.exe", exe));
        assert_eq!(
            registered_executable(r#""C:\Old\alnair-router.exe" --tray"#),
            r"C:\Old\alnair-router.exe"
        );
    }
}
