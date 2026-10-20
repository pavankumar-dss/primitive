#!/bin/sh
# Builds the browser version of primitive into site/.
# Serve it locally with: python3 -m http.server -d site
set -e
cd "$(dirname "$0")/.."

# the engine: the Rust port (needs rustup target add wasm32-unknown-unknown)
(cd engine && cargo build --release --target wasm32-unknown-unknown)
cp engine/target/wasm32-unknown-unknown/release/primitive_engine.wasm site/engine.wasm

# the original Go code, still selectable with ?engine=go for comparison
GOOS=js GOARCH=wasm go build -trimpath -ldflags="-s -w" -o site/primitive.wasm ./cmd/wasm
cp "$(go env GOROOT)/lib/wasm/wasm_exec.js" site/
mkdir -p site/examples
cp examples/*.png site/examples/

# MP4 export muxer (MPL-2.0), pinned and checksummed
MEDIABUNNY=1.61.1
MEDIABUNNY_SHA256=652fbe9226e55cb0c0784aaba6b4b73fea4fc0df498f62050cdc617a4bd97ca2
mkdir -p site/vendor
curl -fsSL -o site/vendor/mediabunny.min.mjs   "https://cdn.jsdelivr.net/npm/mediabunny@$MEDIABUNNY/dist/bundles/mediabunny.min.mjs"
echo "$MEDIABUNNY_SHA256  site/vendor/mediabunny.min.mjs" | sha256sum -c -
