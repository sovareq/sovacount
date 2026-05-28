#!/usr/bin/env bash
# package-macos.sh — Bundle SovaCount.app for macOS distribution.
#
# Modes (set via SIGN_MODE env-var):
#   adhoc        Ad-hoc sign with `codesign --sign -`. Works on the build
#                machine; downloads to other Macs hit Gatekeeper. Default.
#   developer-id Sign with Apple Developer ID Application certificate +
#                hardened-runtime + entitlements. Required for distribution.
#                Requires: SIGNING_IDENTITY env-var (full cert name).
#   notarize     Same as developer-id, then submit to Apple notary service
#                and staple the ticket. Requires: NOTARY_PROFILE
#                (`notarytool store-credentials` profile name).
#
# Output:
#   dist/SovaCount.app          — the signed bundle
#   dist/SovaCount.zip          — zipped bundle (for notarytool / GitHub release)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/SovaCount.app"
SIGN_MODE="${SIGN_MODE:-adhoc}"

echo "[package] root: $ROOT"
echo "[package] sign mode: $SIGN_MODE"

# 1. Clean output.
rm -rf "$DIST"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# 2. Build release binaries.
echo "[package] building governor-http + sovacount-launcher (release)..."
cd "$ROOT"
cargo build --release -p governor-http -p governor-launcher-gui

# 3. Place binaries.
#    Layout chosen so locate_governor_http()'s Resources/-fallback finds the
#    server next to the launcher without needing an env-var.
cp "$ROOT/target/release/sovacount-launcher" "$APP/Contents/MacOS/sovacount-launcher"
cp "$ROOT/target/release/governor-http"      "$APP/Contents/Resources/governor-http"

# 4. Info.plist — copy from LAUNCHER/ template (kept under version control).
cp "$ROOT/LAUNCHER/SovaCount.app/Contents/Info.plist" "$APP/Contents/Info.plist"

# 5. Icon — optional; copy if present.
if [[ -f "$ROOT/LAUNCHER/SovaCount.app/Contents/Resources/icon.icns" ]]; then
  cp "$ROOT/LAUNCHER/SovaCount.app/Contents/Resources/icon.icns" "$APP/Contents/Resources/icon.icns"
fi

# 6. Strip extended attributes that interfere with codesign (quarantine,
#    com.apple.provenance, etc.).
xattr -cr "$APP"

# 7. Codesign.
case "$SIGN_MODE" in
  adhoc)
    echo "[package] ad-hoc signing (local-machine only)..."
    codesign \
      --sign - \
      --deep \
      --force \
      --options runtime \
      --entitlements "$ROOT/crates/governor-launcher-gui/entitlements.plist" \
      "$APP"
    ;;
  developer-id|notarize)
    if [[ -z "${SIGNING_IDENTITY:-}" ]]; then
      echo "[package] ERROR: SIGNING_IDENTITY env-var required for $SIGN_MODE mode."
      echo "          Example: SIGNING_IDENTITY='Developer ID Application: Bjorn Lambrechts (XXXXXXXXXX)'"
      exit 2
    fi
    echo "[package] signing with: $SIGNING_IDENTITY"
    # Sign the server binary FIRST (deep sign would do it too, but explicit
    # is safer when the binary lives under Resources/).
    codesign \
      --sign "$SIGNING_IDENTITY" \
      --options runtime \
      --timestamp \
      --force \
      "$APP/Contents/Resources/governor-http"
    codesign \
      --sign "$SIGNING_IDENTITY" \
      --options runtime \
      --timestamp \
      --deep \
      --force \
      --entitlements "$ROOT/crates/governor-launcher-gui/entitlements.plist" \
      "$APP"
    codesign --verify --strict --verbose=2 "$APP"
    ;;
  *)
    echo "[package] ERROR: unknown SIGN_MODE '$SIGN_MODE' (expected: adhoc|developer-id|notarize)"
    exit 2
    ;;
esac

# 8. Zip for distribution.
ZIP="$DIST/SovaCount.zip"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "[package] bundle: $APP"
echo "[package] zip:    $ZIP"

# 9. Notarize if requested.
if [[ "$SIGN_MODE" == "notarize" ]]; then
  if [[ -z "${NOTARY_PROFILE:-}" ]]; then
    echo "[package] ERROR: NOTARY_PROFILE env-var required for notarize mode."
    echo "          Set up with: xcrun notarytool store-credentials NOTARY_PROFILE ..."
    exit 2
  fi
  echo "[package] submitting to notary service (profile: $NOTARY_PROFILE)..."
  xcrun notarytool submit "$ZIP" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$APP"
  # Re-zip with stapled ticket.
  rm -f "$ZIP"
  ditto -c -k --keepParent "$APP" "$ZIP"
  echo "[package] notarized + stapled: $APP"
fi

echo "[package] done."
