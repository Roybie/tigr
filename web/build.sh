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

# Bake the crate version into a tiny module the worker reads BEFORE the
# wasm loads, so it can cache-bust the wasm URL (tigr_bg.wasm?v=VERSION).
# The version can't come from the wasm itself (it isn't loaded yet), so
# read it from Cargo.toml — the same single source the wasm compiles its
# CARGO_PKG_VERSION in from, so the two can't disagree within a build.
# Written after wasm-pack so its out-dir clean can't remove it.
version=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')
printf 'export const VERSION = "%s";\n' "$version" >web/pkg/meta.js

echo
echo "Playground built. Serve it with, e.g.:"
echo "  python3 -m http.server -d web 8080"
echo "then open http://localhost:8080/"
