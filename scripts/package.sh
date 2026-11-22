#!/usr/bin/env bash
# Build gamma's installable packages for this platform.
#
#   scripts/package.sh                 # app bundles for this OS (macOS: .app + .dmg; Linux: AppImage, .deb, .rpm)
#   GAMMA_CUDA=1 scripts/package.sh    # Linux: the bundled gamma program with NVIDIA GPU support
#
# Environment (all optional; without them the build is unsigned and has no update feed):
#   GAMMA_UPDATE_BASE                  feed base URL compiled into the app: <base>/<channel>/latest.json
#   GAMMA_UPDATER_PUBKEY_FILE          minisign public key the app trusts for updates
#   TAURI_SIGNING_PRIVATE_KEY[_PATH]   matching private key: signs the update archives (.app.tar.gz, .AppImage)
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD
#   APPLE_SIGNING_IDENTITY             "Developer ID Application: ..." to sign the macOS app
#   APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID   notarise (or APPLE_API_KEY/APPLE_API_ISSUER/APPLE_API_KEY_PATH)
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
TARGET=${CARGO_TARGET_DIR:-target}

echo "== interface"
(cd app/ui && npm ci --no-audit --no-fund >/dev/null && npm run build >/dev/null)

echo "== gamma program ($TRIPLE)"
if [ "${GAMMA_CUDA:-0}" = "1" ]; then scripts/build-cuda.sh >/dev/null; else cargo build --release -p gamma-cli >/dev/null; fi
mkdir -p app/src-tauri/binaries
EXT=""; [[ "$TRIPLE" == *windows* ]] && EXT=".exe"
cp "$TARGET/release/gamma$EXT" "app/src-tauri/binaries/gamma-$TRIPLE$EXT"

echo "== app bundles"
if [ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ] && [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  export TAURI_SIGNING_PRIVATE_KEY="$(cat "$TAURI_SIGNING_PRIVATE_KEY_PATH")"
fi
CONFIG='{}'
if [ -n "${GAMMA_UPDATER_PUBKEY_FILE:-}" ]; then
  KEY=$(tr -d '\n' < "$GAMMA_UPDATER_PUBKEY_FILE")
  INSECURE=false
  case "${GAMMA_UPDATE_BASE:-}" in http://127.0.0.1*|http://localhost*) INSECURE=true; echo "   (test build: plain http allowed for a feed on this machine)";; esac
  CONFIG=$(printf '{"plugins":{"updater":{"pubkey":"%s","dangerousInsecureTransportProtocol":%s}}}' "$KEY" "$INSECURE")
else
  echo "   (no GAMMA_UPDATER_PUBKEY_FILE: the app will refuse every update)"
  CONFIG='{"bundle":{"createUpdaterArtifacts":false}}'
fi
case "$(uname -s)" in
  Darwin) BUNDLES="app,dmg" ;;
  Linux) BUNDLES="appimage,deb,rpm" ;;
  *) BUNDLES="nsis" ;;
esac
VERSION=${GAMMA_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
(cd app && CI=${CI:-true} ui/node_modules/.bin/tauri build --bundles "$BUNDLES" --config "$CONFIG" --config "{\"version\":\"$VERSION\"}")
case "$TARGET" in /*) OUT="$TARGET/release/bundle";; *) OUT="$ROOT/$TARGET/release/bundle";; esac
echo "== built"
find "$OUT" -maxdepth 2 \( -name "*.dmg" -o -name "*.app" -o -name "*.tar.gz" -o -name "*.sig" -o -name "*.AppImage" -o -name "*.deb" -o -name "*.rpm" -o -name "*.exe" \) | sed "s|$ROOT/||"
