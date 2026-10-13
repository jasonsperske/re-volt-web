#!/usr/bin/env bash
# Builds the static site into web/ (deploy that directory as-is).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/revolt_web.wasm
if command -v wasm-opt >/dev/null; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int \
    web/pkg/revolt_web_bg.wasm -o web/pkg/revolt_web_bg.wasm
fi
echo "Built web/ ($(du -h web/pkg/revolt_web_bg.wasm | cut -f1) wasm)"
