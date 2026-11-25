#!/usr/bin/env bash
# Everything that must pass before a merge. Run from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test --workspace
(cd crates/jmk-embassy && cargo test)
(cd firmware/rp2040 && cargo build --release)
cargo run -q -p jmk-uf2 -- rp2040 \
    firmware/rp2040/target/thumbv6m-none-eabi/release/jmk-rp2040 target/jmk-rp2040.uf2
# Builds only: no nRF52840 board has run this firmware yet.
(cd firmware/nrf52840 && cargo build --release)
cargo run -q -p jmk-uf2 -- nrf52840 \
    firmware/nrf52840/target/thumbv7em-none-eabihf/release/jmk-nrf52840 target/jmk-nrf52840.uf2

(cd app && npm install --no-audit --no-fund --silent && npm test --silent \
    && npm run e2e --silent -- --reporter=dot)
echo "all checks passed; firmware images at target/jmk-rp2040.uf2 and target/jmk-nrf52840.uf2"
