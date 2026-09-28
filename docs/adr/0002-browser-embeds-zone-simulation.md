# Browser demo embeds the shared zone simulation

Status: accepted for the Greyhaven Vale starter-zone slice.

## Context

The GitHub Pages demo moves a character with its own TypeScript rules in an illustrative scene. That was acceptable while the demo showed only presentation. A starter zone with combat, creature AI, quests, loot and progression cannot keep two rule owners (Rust core for hosts, TypeScript for Pages) without diverging (DESIGN-006). Pages cannot host a zone server, so the public demo cannot rely on a network host.

## Decision

Compile `mmorpg-core` and `mmorpg-protocol` to WebAssembly through a thin `mmorpg-wasm` adapter crate. In the browser it acts as a **local single-player zone host**:

- it constructs the same `ZoneSimulation` with the same content revision as `mmorpg-zone-host`;
- the browser sends encoded commands with increasing sequences and advances fixed 30 Hz ticks from a bounded accumulator;
- the browser reads only encoded **player-scoped** projections and decodes them with the same TypeScript decoder an online WebTransport source would use. Canonical snapshots never cross into rendering;
- presentation-only scenery (`mmorpg-scenery`) is exported through the same module so both clients draw identical worlds.

This is not MMO authority in the browser. It has no fencing, no leases, no handoff, no persistence authority and no shared world. It is the same single-writer simulation running in-process for one local player, like a listen server. The presentation layer cannot tell a local source from a network source, which keeps a future browser online mode a transport swap.

## Consequences

- There is one implementation of gameplay rules, and the browser demo shows the real simulation.
- The Pages build needs a Rust toolchain, the `wasm32-unknown-unknown` target and a pinned `wasm-bindgen` CLI. Generated WASM and bindings are build outputs, not committed (REP-003).
- `mmorpg-core` and `mmorpg-protocol` must keep compiling for `wasm32-unknown-unknown`: no threads, filesystem or wall clock in core paths. CI builds the WASM target.
- Browser tests that need the simulation build the WASM module first; pure decoder and UI tests stay independent.
- Local demo saves persist a durable character record through a core query/command API, not by reaching into simulation internals.
