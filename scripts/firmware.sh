#!/usr/bin/env bash
# Builds the firmware for every chip and puts the files where the app run
# from source looks for them (app/firmware). Run from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

# A system-wide cargo (Homebrew) has no targets for the chips; the one of
# rustup's own toolchain does.
if toolchain=$(rustup which cargo 2>/dev/null); then
  PATH="$(dirname "$toolchain"):$PATH"
  export RUSTC="$(dirname "$toolchain")/rustc"
fi

build() { # chip, target
  (cd "firmware/$1" && cargo build --release)
  cargo run -q -p jmk-uf2 -- "$1" "firmware/$1/target/$2/release/jmk-$1" "target/jmk-$1.uf2"
}
build rp2040 thumbv6m-none-eabi
build rp2350 thumbv8m.main-none-eabihf
build nrf52840 thumbv7em-none-eabihf

mkdir -p app/firmware
cp target/jmk-*.uf2 app/firmware/
