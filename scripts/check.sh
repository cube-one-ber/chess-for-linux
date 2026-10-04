#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --locked -- --render-check
if [ "${CHESS_GUI_CHECK:-0}" = 1 ]; then
    python3 scripts/kirigami_check.py
    python3 scripts/integration_check.py
fi
