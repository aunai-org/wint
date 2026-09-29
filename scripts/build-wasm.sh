#!/usr/bin/env bash
# Builds the JavaScript/WASM package into ./pkg (wint.js, wint_bg.wasm, wint.d.ts).
#
# Prerequisites (one-time):
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.129 --locked   # must match the wasm-bindgen crate
#
# Usage: scripts/build-wasm.sh [out-dir]
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-pkg}"

cargo build --release --lib --target wasm32-unknown-unknown --features wasm
rm -rf "$OUT"
wasm-bindgen --target web --out-dir "$OUT" --out-name wint \
  target/wasm32-unknown-unknown/release/env_operability.wasm

# Optional extra shrink if binaryen is installed.
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz "$OUT/wint_bg.wasm" -o "$OUT/wint_bg.wasm"
fi
ls -l "$OUT"
