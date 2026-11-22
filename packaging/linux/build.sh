#!/usr/bin/env bash
# Build gamma's Linux packages in Docker on this machine (x86_64 or aarch64):
#   packaging/linux/build.sh                # unsigned, no update key
#   GAMMA_UPDATER_PUBKEY_FILE=... TAURI_SIGNING_PRIVATE_KEY_PATH=... GAMMA_UPDATE_BASE=... packaging/linux/build.sh
# Output: target-linux/release/bundle/{appimage,deb,rpm}/
set -euo pipefail
cd "$(dirname "$0")/../.."
IMAGE=gamma-linux-build:$(uname -m)
docker build -q -t "$IMAGE" packaging/linux >/dev/null
ENVS=(-e GAMMA_CUDA=${GAMMA_CUDA:-1} -e GAMMA_VERSION=${GAMMA_VERSION:-} -e GAMMA_UPDATE_BASE=${GAMMA_UPDATE_BASE:-} -e CI=true -e HOME=/tmp)
mkdir -p "$HOME/.cache/gamma-docker/registry"
MOUNTS=(-v "$PWD:/src" -v "$HOME/.cache/gamma-docker/registry:/opt/cargo/registry")
if [ -n "${GAMMA_UPDATER_PUBKEY_FILE:-}" ]; then MOUNTS+=(-v "$GAMMA_UPDATER_PUBKEY_FILE:/keys/updater.pub:ro"); ENVS+=(-e GAMMA_UPDATER_PUBKEY_FILE=/keys/updater.pub); fi
if [ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]; then MOUNTS+=(-v "$TAURI_SIGNING_PRIVATE_KEY_PATH:/keys/updater.key:ro"); ENVS+=(-e TAURI_SIGNING_PRIVATE_KEY_PATH=/keys/updater.key -e TAURI_SIGNING_PRIVATE_KEY_PASSWORD=${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}); fi
# separate target folder so host and container builds do not mix
docker run --rm --user "$(id -u):$(id -g)" "${MOUNTS[@]}" "${ENVS[@]}" -e CARGO_TARGET_DIR=/src/target-linux "$IMAGE" scripts/package.sh
