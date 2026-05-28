#!/usr/bin/env bash
# install.sh — One-line installer for SovaCount.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/sovareq/sovacount/main/scripts/install.sh | bash
#
# Or:
#   curl -fsSL https://raw.githubusercontent.com/sovareq/sovacount/main/scripts/install.sh | bash -s -- --version v0.4.0
#
# Behaviour:
#   - Detect OS (macos / linux / windows-WSL) and arch (arm64 / x86_64).
#   - Fetch the matching archive from the latest GitHub Release (or a
#     specific tag with --version).
#   - Verify the SHA-256 checksum.
#   - Extract to $HOME/.local/bin (override with INSTALL_DIR=...).
#   - On macOS: also place SovaCount.app in $HOME/Applications and strip
#     the quarantine xattr (no Apple Developer ID — this is the friction-
#     free path for an ad-hoc-signed bundle distributed via GitHub).
#
# Exit codes:
#   0  installed successfully
#   1  unsupported platform
#   2  network / download failure
#   3  checksum mismatch (corrupted / tampered download)
#   4  extraction failure

set -euo pipefail

REPO="sovareq/sovacount"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
APP_DIR="${APP_DIR:-$HOME/Applications}"
VERSION=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version|-v) VERSION="$2"; shift 2 ;;
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    --app-dir) APP_DIR="$2"; shift 2 ;;
    --help|-h)
      sed -n '2,30p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

say()  { printf "\033[1;34m[sovacount]\033[0m %s\n" "$*"; }
warn() { printf "\033[1;33m[sovacount]\033[0m %s\n" "$*" >&2; }
die()  { printf "\033[1;31m[sovacount]\033[0m %s\n" "$*" >&2; exit "${2:-1}"; }

# ---------------------------------------------------------------- detect
detect_platform() {
  local os arch target
  case "$(uname -s)" in
    Darwin) os="apple-darwin" ;;
    Linux)  os="unknown-linux-gnu" ;;
    MINGW*|MSYS*|CYGWIN*) os="pc-windows-msvc" ;;
    *) die "unsupported OS: $(uname -s)" 1 ;;
  esac
  case "$(uname -m)" in
    arm64|aarch64) arch="aarch64" ;;
    x86_64|amd64)  arch="x86_64" ;;
    *) die "unsupported arch: $(uname -m)" 1 ;;
  esac
  # Linux + aarch64 isn't built yet (no GitHub-hosted aarch64-linux runner).
  if [[ "$os" == "unknown-linux-gnu" && "$arch" == "aarch64" ]]; then
    die "Linux aarch64 not yet shipped — build from source via \`cargo build --release\`" 1
  fi
  target="${arch}-${os}"
  echo "$target"
}

# ---------------------------------------------------------------- helpers
fetch_latest_tag() {
  # Use GitHub's redirect-to-latest behaviour to avoid hitting the API
  # rate-limit on anonymous clients. `curl -ILs` follows the redirect and
  # prints the final URL, which contains `/tag/vX.Y.Z`.
  local url
  url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
    "https://github.com/$REPO/releases/latest") || die "could not resolve latest release" 2
  echo "${url##*/}"
}

verify_sha256() {
  local file="$1" expected_file="$2"
  local expected actual
  expected=$(awk '{print $1}' "$expected_file")
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$file" | awk '{print $1}')
  else
    actual=$(shasum -a 256 "$file" | awk '{print $1}')
  fi
  [[ "$actual" == "$expected" ]] || die "SHA-256 mismatch for $file" 3
}

# ---------------------------------------------------------------- main
TARGET=$(detect_platform)
say "detected platform: $TARGET"

if [[ -z "$VERSION" ]]; then
  VERSION=$(fetch_latest_tag)
  say "latest release: $VERSION"
fi

CLI_ARCHIVE="sovacount-${VERSION}-${TARGET}.tar.gz"
if [[ "$TARGET" == *"pc-windows-msvc"* ]]; then
  CLI_ARCHIVE="sovacount-${VERSION}-${TARGET}.zip"
fi

BASE_URL="https://github.com/$REPO/releases/download/${VERSION}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

say "downloading $CLI_ARCHIVE..."
curl -fsSL --retry 3 -o "$TMP/$CLI_ARCHIVE" "$BASE_URL/$CLI_ARCHIVE" \
  || die "download failed: $BASE_URL/$CLI_ARCHIVE" 2
curl -fsSL --retry 3 -o "$TMP/$CLI_ARCHIVE.sha256" "$BASE_URL/$CLI_ARCHIVE.sha256" \
  || die "download failed: $BASE_URL/$CLI_ARCHIVE.sha256" 2

say "verifying SHA-256..."
verify_sha256 "$TMP/$CLI_ARCHIVE" "$TMP/$CLI_ARCHIVE.sha256"

say "extracting..."
if [[ "$CLI_ARCHIVE" == *.zip ]]; then
  unzip -q -d "$TMP/extracted" "$TMP/$CLI_ARCHIVE" || die "unzip failed" 4
else
  mkdir -p "$TMP/extracted"
  tar -C "$TMP/extracted" -xzf "$TMP/$CLI_ARCHIVE" || die "tar extract failed" 4
fi
EXTRACTED_DIR="$TMP/extracted/sovacount-${VERSION}-${TARGET}"

mkdir -p "$INSTALL_DIR"
for bin in tier-classify governor-http governor-mcp; do
  exe="$bin"
  [[ "$TARGET" == *"pc-windows-msvc"* ]] && exe="${bin}.exe"
  if [[ -f "$EXTRACTED_DIR/$exe" ]]; then
    install -m 0755 "$EXTRACTED_DIR/$exe" "$INSTALL_DIR/$exe"
    say "installed $INSTALL_DIR/$exe"
  else
    warn "missing in archive: $exe"
  fi
done

# -------------------------------------------------------------- macOS .app
if [[ "$TARGET" == *"apple-darwin"* ]]; then
  APP_ZIP="SovaCount-${VERSION}-${TARGET}.app.zip"
  say "downloading $APP_ZIP..."
  if curl -fsSL --retry 3 -o "$TMP/$APP_ZIP" "$BASE_URL/$APP_ZIP"; then
    curl -fsSL --retry 3 -o "$TMP/$APP_ZIP.sha256" "$BASE_URL/$APP_ZIP.sha256" \
      || warn "could not fetch .app SHA — skipping checksum verify"
    if [[ -f "$TMP/$APP_ZIP.sha256" ]]; then
      verify_sha256 "$TMP/$APP_ZIP" "$TMP/$APP_ZIP.sha256"
    fi
    mkdir -p "$APP_DIR"
    rm -rf "$APP_DIR/SovaCount.app"
    ditto -x -k "$TMP/$APP_ZIP" "$APP_DIR" || die "ditto extract failed" 4
    # Strip the browser/curl quarantine xattr so Gatekeeper doesn't
    # second-guess the ad-hoc signature. This is the trade-off for not
    # paying for an Apple Developer ID.
    xattr -dr com.apple.quarantine "$APP_DIR/SovaCount.app" 2>/dev/null || true
    say "installed $APP_DIR/SovaCount.app"
    say "open it via: open '$APP_DIR/SovaCount.app'"
  else
    warn "no .app bundle for $TARGET in this release — CLI binaries only"
  fi
fi

# -------------------------------------------------------------- PATH hint
if ! command -v tier-classify >/dev/null 2>&1; then
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
      warn "$INSTALL_DIR is not in your \$PATH. Add this to your shell rc:"
      warn "    export PATH=\"\$HOME/.local/bin:\$PATH\""
      ;;
  esac
fi

say "done. Try: tier-classify --task \"Fix typo in README\""
