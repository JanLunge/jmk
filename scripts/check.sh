#!/usr/bin/env bash
# Everything that must pass before a merge. Run from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test --workspace
(cd server && cargo test)
(cd firmware/rp2040 && cargo build --release)
cargo run -q -p jmk-uf2 -- rp2040 \
    firmware/rp2040/target/thumbv6m-none-eabi/release/jmk-rp2040 target/jmk-rp2040.uf2

# The app's registry test talks to a real server on a throwaway database.
(cd server && cargo build)
registry_db=$(mktemp -d)
JMK_BIND=127.0.0.1:8799 JMK_DB="$registry_db/registry.db" server/target/debug/jmk-registry &
registry_pid=$!
trap 'kill "$registry_pid" 2>/dev/null; rm -rf "$registry_db"' EXIT
(cd app && npm install --no-audit --no-fund --silent && npm test --silent \
    && JMK_REGISTRY=http://127.0.0.1:8799 npm run e2e --silent -- --reporter=dot)
echo "all checks passed; firmware image at target/jmk-rp2040.uf2"
