#!/bin/sh
# Install alnair-router from GitHub Releases.
#
#   curl -fsSL https://raw.githubusercontent.com/xFlawlessDev/alnair-router/main/install.sh | sh
#
# Environment overrides:
#   ALNAIR_ROUTER_REPO            GitHub repo slug (default: xFlawlessDev/alnair-router)
#   ALNAIR_ROUTER_VERSION         release tag to install (default: the latest release)
#   ALNAIR_ROUTER_INSTALL_DIR     where the binary lands (default: ~/.local/bin)
#   ALNAIR_ROUTER_NO_AUTOSTART=1  skip `alnair-router install` (auto-start registration)

set -eu

REPO="${ALNAIR_ROUTER_REPO:-xFlawlessDev/alnair-router}"
INSTALL_DIR="${ALNAIR_ROUTER_INSTALL_DIR:-$HOME/.local/bin}"
BINARY="alnair-router"

info() { printf '%s\n' "$*"; }
fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

os=$(uname -s)
arch=$(uname -m)

case "$os" in
  Linux)
    case "$arch" in
      x86_64 | amd64) target="x86_64-unknown-linux-gnu" ;;
      aarch64 | arm64) target="aarch64-unknown-linux-gnu" ;;
      *) fail "unsupported Linux architecture: $arch (build from source with cargo build --release -p alnair-router)" ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      arm64) target="aarch64-apple-darwin" ;;
      *) fail "unsupported macOS architecture: $arch (only Apple Silicon builds are published)" ;;
    esac
    ;;
  *)
    fail "unsupported OS: $os (on Windows: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex)"
    ;;
esac

if [ -n "${ALNAIR_ROUTER_VERSION:-}" ]; then
  tag="$ALNAIR_ROUTER_VERSION"
else
  resolved=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
    || fail "cannot resolve the latest release for $REPO (set ALNAIR_ROUTER_VERSION to pin one)"
  tag=${resolved##*/tag/}
  case "$tag" in
    v[0-9]*) ;;
    *) fail "could not determine the latest release tag (got '$tag')" ;;
  esac
fi

archive="$BINARY-$tag-$target.tar.gz"
url="https://github.com/$REPO/releases/download/$tag/$archive"
sums_url="https://github.com/$REPO/releases/download/$tag/SHA256SUMS.txt"

tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t alnair-router)
trap 'rm -rf "$tmpdir"' EXIT INT TERM

info "Downloading $archive"
curl -fsSL "$url" -o "$tmpdir/$archive" || fail "download failed: $url"

if curl -fsSL "$sums_url" -o "$tmpdir/SHA256SUMS.txt" 2>/dev/null; then
  if command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1; then
    if (cd "$tmpdir" && (sha256sum -c --ignore-missing SHA256SUMS.txt 2>/dev/null || shasum -a 256 -c SHA256SUMS.txt)) >/dev/null 2>&1; then
      info "Checksum verified"
    else
      fail "checksum mismatch for $archive — refusing to install"
    fi
  else
    info "warning: no sha256sum/shasum available; skipping checksum verification"
  fi
else
  info "warning: SHA256SUMS.txt is unavailable; skipping checksum verification"
fi

tar -xzf "$tmpdir/$archive" -C "$tmpdir"
[ -f "$tmpdir/$BINARY" ] || fail "archive did not contain $BINARY"

mkdir -p "$INSTALL_DIR"
cp "$tmpdir/$BINARY" "$INSTALL_DIR/$BINARY"
chmod 755 "$INSTALL_DIR/$BINARY"

info "Installed $BINARY $tag to $INSTALL_DIR/$BINARY"

if [ "${ALNAIR_ROUTER_NO_AUTOSTART:-}" = "1" ]; then
  info "auto-start registration skipped (ALNAIR_ROUTER_NO_AUTOSTART=1)"
else
  "$INSTALL_DIR/$BINARY" install
fi

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) info "note: add $INSTALL_DIR to your PATH, e.g. export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac

info "Uninstall later with: $INSTALL_DIR/$BINARY uninstall"
