#!/usr/bin/env bash
# Run scripts/test-update.sh inside the Linux build container (AppImage update
# flow under a virtual display). GAMMA_TEST_KEY: a test minisign private key.
set -euo pipefail
cd "$(dirname "$0")/../.."
IMAGE=gamma-linux-build:$(uname -m)
docker build -q -t "$IMAGE" packaging/linux >/dev/null
KEY=${GAMMA_TEST_KEY:?set GAMMA_TEST_KEY}
mkdir -p "$HOME/.cache/gamma-docker/registry"
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/src" -v "$HOME/.cache/gamma-docker/registry:/opt/cargo/registry" \
  -v "$KEY:/keys/test.key:ro" -v "$KEY.pub:/keys/test.key.pub:ro" \
  -e GAMMA_TEST_KEY=/keys/test.key -e CARGO_TARGET_DIR=/src/target-linux -e GAMMA_CUDA=${GAMMA_CUDA:-0} -e CI=true -e HOME=/tmp \
  "$IMAGE" xvfb-run -a scripts/test-update.sh
