# First asset-tooling avatar

The browser Ranger uses asset-tooling's `medieval.archer` OBJ at revision
`d0585174de734ba3e0ff407197dcdaa06a252c4d`. The deliberately packaged
`web/assets/medieval-character-kit/` contains the OBJ, its selected SHA-256
manifest entry, the material palette and archer part bindings, and the upstream
revision/recipe identity. The mesh is static; the client moves it with the
projected position, yaw and locomotion bob. The existing hat and hair overlay
still follows the local pose because the first kit provides no headwear.
Warden and Arcanist retain their existing animated procedural models. The
character selection preview and native client are separate presentation paths.

To reproduce from a clean asset-tooling checkout at the declared revision,
with that checkout beside MMORPG, run from the MMORPG root:

```sh
bun web/scripts/package-archer.ts ../asset-tooling --check
```

Use `--write` to reconcile the packaged outputs after an intentional source
revision change. The check re-executes asset-tooling's generator and compares
the OBJ and manifest bytes; it rejects a dirty source checkout and does not
rewrite packaged files. The ordinary
web test verifies the committed OBJ's SHA-256, byte length, group order, mesh
topology and renderer resource keys without requiring the sibling checkout.
The OBJ uses right-handed Y-up millimeters, lowered exactly once to
`IndexedMeshGeometry` in meters. Its content hash is part of every geometry
resource key, so changing bytes cannot reuse an old renderer resource.

This package is client presentation only. The asset does not define gameplay,
physics collision, or authoritative appearance in the network projection.
