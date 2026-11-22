#!/usr/bin/env bash
# Build the gamma program with NVIDIA GPU support (Linux, CUDA 13).
# cuda-core generates its bindings with libclang; point it at the compiler's
# own headers when libclang does not find them by itself.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -z "${BINDGEN_EXTRA_CLANG_ARGS:-}" ]; then
  inc=$(ls -d /usr/lib/gcc/"$(uname -m)"-linux-gnu/*/include 2>/dev/null | sort -V | tail -1 || true)
  [ -n "$inc" ] && export BINDGEN_EXTRA_CLANG_ARGS="-I$inc"
fi
cargo build --release -p gamma-cli --features cuda "$@"
echo "built target/release/gamma (CUDA)"
