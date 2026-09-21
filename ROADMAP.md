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
- [x] Fence reference runtime operations against current directory authority and independent lease time.
- [x] Prove reassignment rejects old-owner commands, ticks, admission and publication.
- Add locally expiring permits and lease-aware WebTransport serving; the current binary remains standalone.
- Persist active deadlines as well as epoch floors and prove partition/crash behavior before failover automation.

## Cross-zone handoff

- Define the authoritative transferable player/character payload separately from connection identity.
- Prepare at a source tick boundary, admit idempotently at the destination, then retire the source.
- Route reconnects and retries through the same transfer ID.
- [x] Prove stale source/destination epochs cannot complete metadata transitions.
- [x] Keep transfer identity stable across lease renewal and reserve one active transfer per entity.
- Separate staged destination acceptance from active simulation and prove source retirement.
- Add explicit fenced cancellation/reconciliation and durable terminal-record retention.
- Add failure tests for source crash, destination crash and duplicate delivery at each phase.

## Interest management and crowded zones

- [x] Add a spatial index owned by the zone simulation.
- [x] Keep player-scoped projection authoritative in core.
- [x] Measure visibility work and snapshot bytes with deterministic sparse, dense and outpost workloads.
- Benchmark physics work independently in collision-heavy and crowded workloads.
- Add dynamic zone subdivision or instancing only when workload evidence shows the static-zone model is insufficient.

## Durable world state

- Separate account/character identity from zone-local connection/player IDs.
- Add command/query interfaces for durable character state.
- Add versioned checkpoint persistence for zone/world state where needed.
- Keep persistence asynchronous to the hot simulation loop.
- Do not introduce event sourcing unless replay/audit requirements justify it beyond existing game-server replay/recovery.

## Physics and gameplay

- [x] Install validated immutable static collision content through the shared physics engine.
- [x] Preserve velocity, gravity and collision content through canonical recovery.
- [x] Verify grounded/wall contact and airborne recovery continuation.
- [x] Load a shared revisioned outpost into the host and native renderer.
- Add authored mesh/material/animation assets through the existing content foundations.
- Introduce durable character identity before live cross-zone gameplay.
- Add server-owned interaction/ability rules with replay-complete cooldown, resource and target state.
- Measure collision-heavy and crowded-zone workloads before changing capacity.

## Client foundations

- [x] Reuse pinned `3d-lab` rendering primitives.
- [x] Decode player-visible snapshots with cross-language golden compatibility evidence.
- [x] Separate fixed demo ticks from interpolated render frames and bound presentation history.
- [x] Connect a native wgpu/winit client to the shared WebTransport session protocol.
- [x] Verify two clients share authoritative movement and key-release acknowledgements.
- [x] Resume native sessions with retained player identity, increasing sequences and presentation reset.
- Add account authentication and live zone routing; current sessions are anonymous.
- [x] Share immutable collision geometry and player dimensions between the server and native client.
- Extend this contract to authored visual assets and their provenance.
- Consume pinned `3d-lab` procedural skeletal animation for two-bone IK, foot placement/locking, pelvis correction and surface-normal alignment against client-visible shared collision geometry; it remains presentation over server-owned movement and physics truth.
- Add bounded motion warping for interactions and attacks only from server-owned target/cue data; warped presentation must not change authoritative transforms, hits, cooldowns, recovery, zone handoff or replay state.
- Add optional shared-physics prediction and acknowledgement-based reconciliation.
- Reuse `input-bindings` for bindings and `settings` for user configuration.
- Reuse `asset-tooling` for processed authored assets and provenance.
- Integrate social functionality with `social-service` only at the boundary that service actually owns.

## Desktop delivery

- [x] Native wgpu game window with interpolated authoritative snapshots.
- [x] Bounded network state, TLS trust, timeout and shutdown ownership.
- [x] One bounded automatic resume attempt on transport interruption, with cancellable shutdown.
- [x] Reproducible local host/client/GPU smoke harness.
- Add Tauri only when launcher, login, patching and settings workflows need a webview shell.
- Add platform packaging and Windows/macOS verification before distributing installers.
- Extend shared transport for snapshots larger than the negotiated datagram budget.
