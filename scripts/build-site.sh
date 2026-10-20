#!/bin/sh
# Builds the browser version of primitive into site/.
# Serve it locally with: python3 -m http.server -d site
set -e
cd "$(dirname "$0")/.."

# the engine (needs rustup target add wasm32-unknown-unknown)
cargo build --release --target wasm32-unknown-unknown -p primitive-engine
cp target/wasm32-unknown-unknown/release/primitive_engine.wasm site/engine.wasm

mkdir -p site/examples
cp examples/*.png site/examples/

# Third-party files, pinned and checksummed, so the site needs no other
# servers. Downloads are skipped when the file is already there and intact.
fetch() { # url path sha256
  mkdir -p "$(dirname "$2")"
  if ! echo "$3  $2" | sha256sum -c --status - 2>/dev/null; then
    curl -fsSL -o "$2" "$1"
    echo "$3  $2" | sha256sum -c -
  fi
}

# MP4 export muxer (MPL-2.0)
MEDIABUNNY=1.61.1
fetch "https://cdn.jsdelivr.net/npm/mediabunny@$MEDIABUNNY/dist/bundles/mediabunny.min.mjs"   site/vendor/mediabunny.min.mjs 652fbe9226e55cb0c0784aaba6b4b73fea4fc0df498f62050cdc617a4bd97ca2

# Subject selection: SlimSAM (Apache-2.0), run by Transformers.js
# (Apache-2.0) on ONNX Runtime's CPU build (MIT), about 28 MB, loaded only
# when someone selects a subject
TRANSFORMERS=4.3.1
ORT=1.31.0-dev.20260914-8d85527a0 # the version this Transformers.js uses
SLIMSAM=5850ab45f587c112167512ffef949107115e26a0 # a commit of Xenova/slimsam-77-uniform
fetch "https://cdn.jsdelivr.net/npm/@huggingface/transformers@$TRANSFORMERS/dist/transformers.min.js"   site/vendor/transformers/transformers.min.js 8d6716d9086f57c30a4bf367dba61b887593573c770c454465e8019b2703e743
for f in ort-wasm-simd-threaded.mjs:c57ca56328877353a575e51bbca6f18450027d6c9bf2307a2cb2c41363b4de9f          ort-wasm-simd-threaded.wasm:06ba057753da3847e4c24f02d91ab133455b0817c69a44993a9a53a2146df9e3; do
  fetch "https://cdn.jsdelivr.net/npm/onnxruntime-web@$ORT/dist/${f%%:*}" "site/vendor/ort/${f%%:*}" "${f#*:}"
done
for f in config.json:6339884f168658d3ca6473b486973913fb33e84e625e06ae2dd7b4a808187419          preprocessor_config.json:225545a743c654e3c495ec6f545a0eaba57c8ba3fbbd8483b3cb1c0fc58db517          onnx/vision_encoder_quantized.onnx:cce23c7b2e5d4f330932738fb67ba518e04b0d99ccdd1cccd22a7da4e01f2971          onnx/prompt_encoder_mask_decoder_quantized.onnx:cb90b279f549d2cab7fd6e20c38522438c65d84bdcca3d2a764cff7d857fdce2; do
  fetch "https://huggingface.co/Xenova/slimsam-77-uniform/resolve/$SLIMSAM/${f%%:*}"     "site/vendor/models/Xenova/slimsam-77-uniform/${f%%:*}" "${f#*:}"
done
