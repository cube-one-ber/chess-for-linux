#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --manifest-path wasm/Cargo.toml --release --locked --target wasm32-wasip1
python3 scripts/package_wasm.py "$@"
