# Saved Greyhaven relief authoring

The Outpost package in `crates/mmorpg-scenery/assets/outpost-relief/` freezes
the current shared presentation heights and a separately editable flatten mask.
This authoring package does not yet change live scenery. Issue #98 owns shared
client adoption and visual acceptance. Gameplay, colliders and snapshots retain
their current identities.

## Source and calibration

`source.heights.json` contains 18,753 signed integer centimetre heights captured
from the public WASM `reliefAt(x,z)` query at MMORPG commit
`6265e1c842aad9d2034bfe7946dec4582158b9e6`. This is the existing shared scenery
output, including its original analytic clearances, rather than an unflattened
noise field. `source.json` records the exact source commit, complete scenery
export SHA-256, presentation fingerprint and gameplay content identity.

To reproduce the capture, build that source revision's browser WASM adapter,
initialize its generated module, and query `reliefAt(-3500 + column * 50,
-1300 + row * 50)` for each row 0…132 and column 0…140. Store the returned
signed integer heights in row-major order with the declared grid metadata.
The saved source is deliberate authoring input; normal package builds never
recapture it from a later live client or alter it during filtering.

The non-square grid has 141 columns along +X and 133 rows along +Z. Its origin
is world XZ = [−3500, −1300] centimetres, step = 50 centimetres, and inclusive
last sample = [3500, 5300]. The center sample is [0, 2000]. Y is centimetres
above the existing flat gameplay ground; it has no world-origin offset.
Source heights range from −36 to +25 cm and must stay within the shared
presentation range −60…+60 cm.

Each signed height is encoded as an opaque grayscale byte `height + 128`.
One byte step equals one centimetre, so encoding loses no precision and the
producer target byte 128 maps exactly to height zero. Height and coverage
channels are explicitly linear scalar data; the RGBA8 container's color tag
does not request a gamma transform.

## Saved coverage and boundaries

`flatten.txt` contains exactly 133 newline-terminated rows of 141 glyphs:
`.` = 0, `1` = 64, `2` = 128, `3` = 192, and `X` = 255. Full coverage extends
1.5 m beyond selected building footprints and the spawn plaza, and 2.5 m from
road center lines. Three half-metre rings soften the exterior transition.
The outer two samples along every edge have zero coverage, preserving source
boundary values for bounded adoption. Changing them fails closed.

The initial saved mask changes 4,517 samples. Zero coverage retains source
heights, full coverage reaches zero, and partial coverage uses the producer's
deterministic integer blend. Production MMORPG code only encodes the inputs
and decodes signed output heights; it contains no flattening kernel.

## Edit and reproduce

Use a clean asset-tooling checkout at
`1e79d74ee0a62cd29706ed3254a0293c91d996ba`, then run from the MMORPG root:

```sh
bun web/scripts/package-relief.ts /absolute/path/to/asset-tooling --check
# After deliberately editing flatten.txt:
bun web/scripts/package-relief.ts /absolute/path/to/asset-tooling --write
(cd web && ASSET_TOOLING_MASK_SOURCE=/absolute/path/to/asset-tooling bun test tests/relief-package.test.ts)
(cd web && bunx --no-install tsc --noEmit -p tsconfig.assets.json)
```

The adapter invokes only the pinned public `image.height.mask-flatten@1`
operation. Grass and relief share the small checkout/public-export boundary;
their authoring pipelines and saved inputs remain independent. Each build
uses disposable content-addressed stores, repeats the operation and compares
a cold replay. Neither `--check` nor `--write` modifies source heights or masks.

The generated source, coverage and flattened RGBA8 containers, plus
`flattened.heights.json`, are checked in. `manifest.json` records every input
and output hash/length, complete AssetRefs, calibration, operation build/source
identity, and hashes of the consumer adapter and its shared helpers.

Ordinary tests validate all package identities, signed-height encoding,
non-square endpoint mapping, retained boundary samples and an independent
test-only blend oracle. With `ASSET_TOOLING_MASK_SOURCE`, tests also reproduce
all bytes through the public producer, edit one mask cell, undo that edit,
exercise zero/full interior coverage and reject invalid pins/calibration.
Pages CI checks out the exact producer, reproduces both saved packages and
runs these cases. The accepted grass selection is unchanged.
