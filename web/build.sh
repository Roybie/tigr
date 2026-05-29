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

# Bake a cache-bust token into a tiny module the worker reads BEFORE the
# wasm loads, so it can stamp the wasm URL (tigr_bg.wasm?v=BUILD_ID). The
# token is the crate version plus a checksum of the built wasm, so it
# changes whenever the wasm bytes change — not only on a version bump —
# which matters because GitHub Pages caches each URL independently and a
# same-version-but-rebuilt wasm at a stale URL is exactly what produced
# "wasm.<export> is not a function". The token can't come from the wasm
# itself (it isn't loaded yet). `cksum` is POSIX, so it works on both the
# macOS dev box and the Linux CI runner. Written after wasm-pack so its
# out-dir clean can't remove it.
version=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')
sum=$(cksum web/pkg/tigr_bg.wasm | cut -d' ' -f1)
printf 'export const BUILD_ID = "%s-%s";\n' "$version" "$sum" >web/pkg/meta.js

echo
echo "Playground built. Serve it with, e.g.:"
echo "  python3 -m http.server -d web 8080"
echo "then open http://localhost:8080/"
