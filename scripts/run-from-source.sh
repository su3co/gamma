#!/usr/bin/env bash
# Build gamma from source and start the desktop app (macOS or Linux).
#
#   scripts/run-from-source.sh            # build (release) and start the app
#   scripts/run-from-source.sh --build    # build only
#
# Needs: Rust (rustup installs the pinned toolchain), Node.js 20+, git; on
# Linux the WebKitGTK 4.1 and GTK 3 development packages (see docs/install.md).
set -euo pipefail
cd "$(dirname "$0")/.."
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
TARGET=${CARGO_TARGET_DIR:-target}
echo "== interface"
(cd app/ui && npm ci --no-audit --no-fund >/dev/null && npm run build >/dev/null)
echo "== gamma program"
cargo build --release -p gamma-cli
# the shell's build checks that its bundled program exists
mkdir -p app/src-tauri/binaries
cp "$TARGET/release/gamma" "app/src-tauri/binaries/gamma-$TRIPLE"
echo "== desktop app"
cargo build --release -p gamma-app
echo "built $TARGET/release/gamma-app and $TARGET/release/gamma"
[ "${1:-}" = "--build" ] && exit 0
exec "$TARGET/release/gamma-app"
