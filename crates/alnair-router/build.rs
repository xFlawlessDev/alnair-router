//! Build-time asset preparation for the router binary.
//!
//! Two jobs, both best-effort so a fresh clone always builds:
//!
//! * the dashboard assets `rust-embed` compiles in — a fresh clone has no
//!   `apps/web/dist`, so we drop a placeholder page explaining how to build it;
//! * the Windows executable icon and version block, compiled from the shared
//!   `assets/alnair-white.ico` artwork into the binary's resource section.

use std::path::{Path, PathBuf};

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>alnair-router</title>
  </head>
  <body style="font-family: system-ui, sans-serif; max-width: 40rem; margin: 4rem auto">
    <h1>Dashboard not built</h1>
    <p>
      Run <code>pnpm install &amp;&amp; pnpm run build</code> in <code>apps/web</code>,
      then rebuild the router.
    </p>
  </body>
</html>
"#;

fn main() {
    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );

    let dist = manifest.join("../../apps/web/dist");
    ensure_dashboard(&dist);

    if target_os() == "windows" {
        embed_windows_resources(&manifest);
    }
}

/// Drops a placeholder dashboard when the real one has not been built.
fn ensure_dashboard(dist: &Path) {
    let index = dist.join("index.html");

    if !index.exists() {
        std::fs::create_dir_all(dist).expect("create apps/web/dist");
        std::fs::write(&index, PLACEHOLDER).expect("write placeholder index.html");
    }

    println!("cargo:rerun-if-changed=../../apps/web/dist");
}

/// Compiles the app icon and version block into the Windows executable.
///
/// The icon is the same artwork the tray uses, so a shortcut, the taskbar and
/// Explorer all show the brand mark; `FileDescription` comes from the package
/// description, which is what Explorer and the Task Manager display.
fn embed_windows_resources(manifest: &Path) {
    let icon = manifest.join("../../assets/alnair-white.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(&icon.to_string_lossy());

    if let Ok(description) = std::env::var("CARGO_PKG_DESCRIPTION") {
        resource.set("FileDescription", &description);
    }

    if let Err(error) = resource.compile() {
        println!("cargo:warning=cannot embed the Windows icon: {error}");
    }
}

/// Target OS taken from cargo, not the host, so cross-builds stay correct.
fn target_os() -> String {
    std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS is set by cargo")
}
