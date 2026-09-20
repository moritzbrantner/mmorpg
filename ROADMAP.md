# MMORPG roadmap

## Foundation — distributed world authority

- [x] Define zone-local deterministic authority around `physics-engine`.
- [x] Adapt zone authority to `game-server::GameSimulation`.
- [x] Use player-scoped snapshots so interest policy stays outside transport.
- [x] Define fenced zone leases and deterministic host routing.
- [x] Define idempotent cross-zone handoff metadata.
- [x] Commit the generated workspace `Cargo.lock` after bootstrap dependency resolution.

## Runnable zone host

- [x] Add a zone-host binary around `game-server::MatchHost`.
- [x] Prove multiple zones advance independently in one process.
- [x] Expose health/readiness/status without making status endpoints gameplay authorities.
- [x] Keep one listener/routing surface per host rather than one process per player or one port per zone.
- [x] Recover the complete hosted zone set atomically across graceful restarts.

## Distributed control plane

- Turn the in-memory reference model into a networked service boundary.
- [x] Add deterministic lease TTL/renewal and fail-closed expiry semantics to the reference model.
- Add host registration/heartbeats and require live host registration for placement.
- Persist lease epochs transactionally so restart cannot resurrect stale ownership.
- Keep the backing store/provider replaceable behind the control-plane contract.
- Add deterministic split-brain/fencing tests before failover automation.

## Cross-zone handoff

- Define the authoritative transferable player/character payload separately from connection identity.
- Prepare at a source tick boundary, admit idempotently at the destination, then retire the source.
- Route reconnects and retries through the same transfer ID.
- Prove stale source/destination epochs cannot complete a transfer.
- Add failure tests for source crash, destination crash and duplicate delivery at each phase.

## Interest management and crowded zones

- Add a spatial index owned by the zone simulation.
- Keep player-scoped projection authoritative in core.
- Benchmark visibility work, snapshot bytes and physics work independently.
- Add dynamic zone subdivision or instancing only when workload evidence shows the static-zone model is insufficient.

## Durable world state

- Separate account/character identity from zone-local connection/player IDs.
- Add command/query interfaces for durable character state.
- Add versioned checkpoint persistence for zone/world state where needed.
- Keep persistence asynchronous to the hot simulation loop.
- Do not introduce event sourcing unless replay/audit requirements justify it beyond existing game-server replay/recovery.

## Client foundations

- [x] Add a small single-player GitHub Pages tech demo that drives `mmorpg-core` locally through WASM.
- [x] Reuse the pinned `3d-lab` renderer instead of creating an MMORPG-local renderer.
- [x] Reuse the `input-bindings` browser runtime for movement/interact controls instead of adding a key resolver.
- [x] Keep initial landmark interactions explicitly non-persistent and outside production world authority.
- Reuse `settings` for user configuration when the client has durable preferences.
- Reuse `asset-tooling` for processed authored assets and provenance.
- Move durable character/world interactions behind authoritative core command/query boundaries before they become gameplay features.
- Add renderer/input/browser runtime evidence once the demo has enough representative work to benchmark.
- Integrate social functionality with `social-service` only at the boundary that service actually owns.
