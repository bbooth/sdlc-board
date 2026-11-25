#!/bin/sh
# Builds the web version of the app into $1 (default dist/): index.html,
# board-app.js and board-app_bg.wasm, which the server serves at /.
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli at the
# wasm-bindgen version in Cargo.lock.
set -eu
out="${1:-dist}"
target="${CARGO_TARGET_DIR:-target}"
cargo build --locked --release -p board-app --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir "$out" "$target/wasm32-unknown-unknown/release/board-app.wasm"
cp "$(dirname "$0")/index.html" "$out/"
