#!/usr/bin/env sh
# Build the tigr playground.
#
# Compiles the VM to WebAssembly and writes the wasm-bindgen glue to
# web/pkg/. The playground is the whole web/ directory — serve it with
# any static file server once this finishes.
#
# Requires: wasm-pack (https://rustwasm.github.io/wasm-pack/) and the
# wasm32-unknown-unknown target (`rustup target add wasm32-unknown-unknown`).
set -e

cd "$(dirname "$0")/.."

wasm-pack build --target web --out-dir web/pkg --no-typescript --no-pack

echo
echo "Playground built. Serve it with, e.g.:"
echo "  python3 -m http.server -d web 8080"
echo "then open http://localhost:8080/"
