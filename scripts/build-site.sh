#!/bin/sh
# Builds the browser version of primitive into site/.
# Serve it locally with: python3 -m http.server -d site
set -e
cd "$(dirname "$0")/.."
GOOS=js GOARCH=wasm go build -trimpath -ldflags="-s -w" -o site/primitive.wasm ./cmd/wasm
cp "$(go env GOROOT)/lib/wasm/wasm_exec.js" site/
mkdir -p site/examples
cp examples/*.png site/examples/
