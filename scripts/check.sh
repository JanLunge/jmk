#!/usr/bin/env bash
# Everything that must pass before a merge. Run from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test --workspace
(cd firmware/rp2040 && cargo build --release)
cargo run -q -p jmk-uf2 -- rp2040 \
    firmware/rp2040/target/thumbv6m-none-eabi/release/jmk-rp2040 target/jmk-rp2040.uf2
echo "all checks passed; firmware image at target/jmk-rp2040.uf2"
