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
# Make ./pkg a valid npm package (not published; run `npm publish` from there when ready).
# The package is named `wint-engine` (`wint` is taken on npm); override with WINT_NPM_NAME, e.g. a scoped name.
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
NAME="${WINT_NPM_NAME:-wint-engine}" VERSION="$VERSION" OUT="$OUT" node -e '
const fs = require("fs");
const pkg = {
  name: process.env.NAME,
  version: process.env.VERSION,
  description: "Deterministic, explainable operability windows from environmental time series (WebAssembly build of wint)",
  type: "module",
  main: "wint.js",
  module: "wint.js",
  types: "wint.d.ts",
  files: ["wint.js", "wint_bg.wasm", "wint_bg.wasm.d.ts", "wint.d.ts", "README.md", "LICENSE"],
  sideEffects: false,
  license: "MIT",
  repository: { type: "git", url: "git+https://github.com/aunai-org/wint.git" },
  keywords: ["weather", "forecast", "operability", "wasm", "scheduling"],
};
fs.writeFileSync(process.env.OUT + "/package.json", JSON.stringify(pkg, null, 2) + "\n");
fs.writeFileSync(process.env.OUT + "/README.md", "# " + pkg.name + "\n\nWebAssembly build of [wint](https://github.com/aunai-org/wint). JSON in, JSON out; see the repository README for the functions and formats.\n\n```js\nimport init, { search, presetPlan } from \"" + pkg.name + "\";\nawait init();\n```\n");
'
cp LICENSE "$OUT/LICENSE"
ls -l "$OUT"
