#!/usr/bin/env bash
# Everything that must pass before a merge. Run from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test --workspace
(cd crates/jmk-embassy && cargo test)
(cd server && cargo test)
(cd firmware/rp2040 && cargo build --release)
cargo run -q -p jmk-uf2 -- rp2040 \
    firmware/rp2040/target/thumbv6m-none-eabi/release/jmk-rp2040 target/jmk-rp2040.uf2
# Builds only: no nRF52840 board has run this firmware yet.
(cd firmware/nrf52840 && cargo build --release)
cargo run -q -p jmk-uf2 -- nrf52840 \
    firmware/nrf52840/target/thumbv7em-none-eabihf/release/jmk-nrf52840 target/jmk-nrf52840.uf2

# The app's registry test talks to a real server on a throwaway database.
(cd server && cargo build)
registry_db=$(mktemp -d)
JMK_BIND=127.0.0.1:8799 JMK_DB="$registry_db/registry.db" server/target/debug/jmk-registry &
registry_pid=$!
trap 'kill "$registry_pid" 2>/dev/null; rm -rf "$registry_db"' EXIT
(cd app && npm install --no-audit --no-fund --silent && npm test --silent \
    && JMK_REGISTRY=http://127.0.0.1:8799 npm run e2e --silent -- --reporter=dot)
echo "all checks passed; firmware images at target/jmk-rp2040.uf2 and target/jmk-nrf52840.uf2"
