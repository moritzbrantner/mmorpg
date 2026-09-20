# Pinned input-bindings browser artifact

`input-bindings-browser.js` is the self-contained browser distribution built from
[`moritzbrantner/input-bindings`](https://github.com/moritzbrantner/input-bindings)
commit `aec9cbfd4de9f3c9af824b2066afd485cccd0da1` with `npm ci --ignore-scripts`
followed by `npm run build:pages`.

Its SHA-256 is
`b29828e529b7cc8d082785e0830eadaa6cbdb7089b07e705a9d00138a21ead84`.
The upstream project is licensed under MIT or Apache-2.0. This checked-in build
artifact keeps the MMORPG Pages demo independent of the mutable GitHub Pages
deployment while leaving input resolution and runtime behavior owned upstream.
