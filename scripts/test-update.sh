#!/usr/bin/env bash
# End-to-end test of the update flow against a local, signed test feed:
#   install version A, publish version B to a feed, update A -> B (download,
#   signature check, install), check probation, roll back to A by hand, update
#   again, then let two "failed" starts trigger the automatic rollback.
#
#   scripts/test-update.sh            # macOS (.app) or Linux (AppImage)
#
# Needs a test signing key pair (never a production key):
#   npx tauri signer generate --ci -p "" -w <dir>/test-updater.key
#   GAMMA_TEST_KEY=<dir>/test-updater.key scripts/test-update.sh
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD
KEY=${GAMMA_TEST_KEY:?set GAMMA_TEST_KEY to a test minisign private key (its .pub next to it)}
WORK=$(cd "$(mktemp -d)" && pwd -P)
PORT=${GAMMA_TEST_FEED_PORT:-18400}
export GAMMA_DATA_DIR="$WORK/data"
SERVER=
trap '[ -n "$SERVER" ] && kill $SERVER 2>/dev/null || true' EXIT

build() { # version
  GAMMA_VERSION=$1 GAMMA_UPDATE_BASE="http://127.0.0.1:$PORT" GAMMA_UPDATER_PUBKEY_FILE="$KEY.pub" \
    TAURI_SIGNING_PRIVATE_KEY_PATH="$KEY" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" scripts/package.sh >"$WORK/build-$1.log" 2>&1 \
    || { tail -30 "$WORK/build-$1.log"; exit 1; }
  echo "== built $1"
}

case "$(uname -s)" in
  Darwin)
    build 0.9.0
    mkdir -p "$WORK/Applications"; ditto target/release/bundle/macos/gamma.app "$WORK/Applications/gamma.app"
    APP="$WORK/Applications/gamma.app/Contents/MacOS/gamma-app"
    build 0.9.1
    ART=target/release/bundle/macos/gamma.app.tar.gz; PLAT=darwin-aarch64 ;;
  Linux)
    build 0.9.0
    T=${CARGO_TARGET_DIR:-target}
    cp "$T"/release/bundle/appimage/*_0.9.0_*.AppImage "$WORK/gamma.AppImage"; chmod +x "$WORK/gamma.AppImage"
    APP="$WORK/gamma.AppImage"; export APPIMAGE="$WORK/gamma.AppImage" APPIMAGE_EXTRACT_AND_RUN=1
    build 0.9.1
    ART=$(ls "$T"/release/bundle/appimage/*_0.9.1_*.AppImage); PLAT="linux-$(uname -m)" ;;
esac
python3 packaging/make-feed.py --version 0.9.1 --channel stable --notes CHANGELOG.md --base "http://127.0.0.1:$PORT" --artifact "$PLAT=$ART" --out "$WORK/feed" >/dev/null
(cd "$WORK/feed" && exec python3 -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1) & SERVER=$!
sleep 1

step() { echo "\$ gamma-app $*"; "$APP" "$@"; }
v() { "$APP" --version-info | python3 -c "import json,sys; print(json.load(sys.stdin)['version'])"; }
fail() { echo "FAILED: $*"; exit 1; }

echo "== installed: $(v)"
step --check-update stable
step --self-update stable
[ "$(v)" = "0.9.1" ] || fail "update did not install"
echo "== after update: 0.9.1, on probation"
step --rollback
[ "$(v)" = "0.9.0" ] || fail "manual rollback"
echo "== manual rollback: 0.9.0"
step --self-update stable >/dev/null
[ "$(v)" = "0.9.1" ] || fail "second update"
# two launches that never report a healthy start (--version-info opens no window):
# the version-info calls above already count; the next start restores the kept version
for i in 1 2 3; do "$APP" --version-info >/dev/null || true; done
[ "$(v)" = "0.9.0" ] || fail "automatic rollback after failed starts"
echo "== automatic rollback after failed starts: 0.9.0"
# a tampered signature is refused
python3 - "$WORK/feed/stable/latest.json" <<'PY'
import json, sys, base64
p = sys.argv[1]; f = json.load(open(p))
for k, v in f["platforms"].items():
    raw = base64.b64decode(v["signature"]).decode().splitlines()
    sig = bytearray(base64.b64decode(raw[1])); sig[-1] ^= 1
    raw[1] = base64.b64encode(bytes(sig)).decode()
    v["signature"] = base64.b64encode(("\n".join(raw) + "\n").encode()).decode()
json.dump(f, open(p, "w"))
PY
out=$("$APP" --self-update stable || true)
echo "$out"
echo "$out" | grep -qi "signature" || fail "a bad signature was not refused"
[ "$(v)" = "0.9.0" ] || fail "bad update changed the installation"
echo "== tampered signature refused, installation unchanged"
echo "ALL UPDATE CHECKS PASSED ($(uname -s) $(uname -m))"
rm -rf "$WORK"
