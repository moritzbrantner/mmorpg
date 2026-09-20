# Browser tech demo

The GitHub Pages surface is deliberately a **single-player technology demo**, not a browser implementation of the distributed production topology.

## Authority boundary

- `mmorpg-core` remains authoritative for the local player simulation. The Pages WASM adapter sends `ZoneCommand::SetMovement` commands into `ZoneSimulation` and advances the same deterministic tick path used by the server foundation.
- `physics-engine` therefore remains authoritative for physical movement. JavaScript never integrates the player position itself.
- `3d-lab` remains the rendering adapter. Pages supplies scene nodes and camera policy but does not create a second renderer implementation.
- `input-bindings` owns keyboard normalization, press/release lifecycle, and binding resolution through its browser bridge. Pages only declares MMORPG-specific semantic actions and maps them to core commands.
- The three landmark interactions are intentionally presentation-only tech-demo progress. They are not persisted and are not a substitute for future durable character/world command boundaries.
- `game-server`, `mmorpg-control-plane`, leases, handoffs, and multiplayer routing are not started by Pages. The demo should remain useful when entirely offline after its static dependencies are loaded.

## Initial scenario

`Frontier Hollow` is a compact exploratory area. The player can move with WASD or the arrow keys and use **E** near three landmarks: a boundary stone, an old well, and a watchfire. Completing them exercises the client interaction seam without pretending that the browser owns production MMORPG persistence.

## Build

The Pages build compiles the small WASM adapter, stages the static client, then lets `github-pages-template` augment the result with shared `/stats/`, `/evidence/`, and preference surfaces. Deployment remains delegated to `reusable-workflows`.
