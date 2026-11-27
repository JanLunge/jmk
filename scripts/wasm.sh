#!/usr/bin/env bash
# Builds the firmware's logic for the browser and puts it where the app loads it.
set -euo pipefail
cd "$(dirname "$0")/.."
# A cargo from Homebrew comes first on many Macs and has no wasm target;
# rustup's own toolchain does.
if toolchain=$(rustup which cargo 2>/dev/null); then PATH="$(dirname "$toolchain"):$PATH"; fi
cargo build -p jmk-wasm --target wasm32-unknown-unknown --release
mkdir -p app/src/wasm
cp target/wasm32-unknown-unknown/release/jmk_wasm.wasm app/src/wasm/jmk.wasm
