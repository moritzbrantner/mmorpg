#!/usr/bin/env bash
set -euo pipefail

cargo build --manifest-path web-wasm/Cargo.toml --target wasm32-unknown-unknown --release --locked

rm -rf dist
mkdir -p dist/pkg
cp -R web/. dist/

wasm-bindgen \
  web-wasm/target/wasm32-unknown-unknown/release/mmorpg_web.wasm \
  --target web \
  --out-dir dist/pkg \
  --out-name mmorpg_web \
  --no-typescript

touch dist/.nojekyll
./node_modules/.bin/github-pages-template build \
  --config ./site/pages.config.json \
  --out ./dist \
  --augment
