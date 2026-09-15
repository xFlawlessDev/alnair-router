//! Ensures the dashboard assets exist before `rust-embed` compiles them in.
//!
//! A fresh clone has no `apps/web/dist`. Rather than failing the Rust build, we
//! drop a placeholder page that tells the operator how to build the dashboard.

use std::path::PathBuf;

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
    let index = dist.join("index.html");

    if !index.exists() {
        std::fs::create_dir_all(&dist).expect("create apps/web/dist");
        std::fs::write(&index, PLACEHOLDER).expect("write placeholder index.html");
    }

    println!("cargo:rerun-if-changed=../../apps/web/dist");
    println!("cargo:rerun-if-changed=build.rs");
}
