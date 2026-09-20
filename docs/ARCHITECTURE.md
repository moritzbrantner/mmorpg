# Architecture

## 1. Core rule: distribute ownership, not one simulation

The world is partitioned into authoritative zones. Each active zone has one writer identified by:

```text
(zone_id, host_id, lease_epoch)
```

`lease_epoch` is a fencing token. Any command that changes ownership or completes a handoff must carry the epoch it observed. A stale host can keep running locally after a partition, but its control-plane operations are rejected once a newer epoch exists.

This is the central split-brain defense. Heartbeats alone are not authority.

## 2. Data plane

A zone runtime is composed vertically:

```text
game-server MatchRuntime
        |
        v
mmorpg-game-server adapter
        |
        v
mmorpg-core ZoneSimulation
        |
        v
physics-engine
```

`game-server` stays responsible for connection/session identity, command sequencing at the runtime boundary, deterministic ticks, reconnect, replay/recovery and WebTransport. `mmorpg-core` owns gameplay interpretation, physics inputs and player-visible interest policy.

One process can host many zones with `game-server::MatchHost`. Zone IDs map deterministically to route-safe match IDs. The MMO repository should extend orchestration around `MatchHost`, not fork its session runtime.

The `mmorpg-zone-host` binary materializes this boundary. It creates one runtime per configured zone, serves all zones through one WebTransport listener and delegates liveness/readiness/status to `game-server` on a separate operational HTTP listener. Status is a projection of host state; it is not a gameplay or placement command surface.

## 3. Snapshot authority

Two projections exist for different purposes:

- **canonical zone snapshot** — complete deterministic state needed by replay, recovery and server-side verification;
- **player-scoped snapshot** — only the state visible/relevant to one authenticated player.

The adapter opts into `SnapshotScope::PlayerScoped`. Transport therefore asks the simulation for the addressed player's projection instead of broadcasting the canonical snapshot. Interest management remains game authority, not transport policy.

The initial projection uses a simple deterministic distance bound. A spatial index can replace the lookup later without moving visibility rules out of core.

## 4. Control plane

The control plane does not tick gameplay. It owns:

- host identity/registration;
- zone placement;
- current zone lease and epoch;
- routing metadata;
- drain/rebalance intent;
- cross-zone handoff metadata.

The first implementation is deliberately in-memory so these semantics can be tested without prematurely selecting etcd, Consul, PostgreSQL, Kubernetes leases or another provider-specific substrate.

A production backing implementation must preserve the same compare-and-set/fencing behavior.

## 5. Zone placement

Static zones are the initial partitioning model. A host advertises capacity; the control plane assigns zones and issues leases. Capacity should consider at least:

- number of zones;
- active/occupied players;
- physics work;
- snapshot/interest work;
- CPU/memory headroom.

No single scalar “players per server” target is encoded yet. Workload evidence should drive those limits.

## 6. Cross-zone handoff

A seamless handoff is a small transaction across two independently authoritative zones:

1. source validates its current lease;
2. source freezes/export the transferable player state at an authoritative tick boundary;
3. a unique transfer ID is prepared with source and destination lease epochs;
4. destination validates its lease and imports the transfer idempotently;
5. client routing moves to the destination session;
6. source retires the transferred entity only after destination acceptance;
7. transfer metadata becomes committed.

Retries of prepare/accept/commit are safe for the same transfer. Reusing a transfer ID with different content fails closed. A stale source or destination epoch cannot advance the transfer.

The first slice implements only the control metadata state machine. Actual player-state export/import and live network rerouting are intentionally next-slice work.

## 7. Persistence

Durable account/character/world data is not owned by the zone transport and should not be written synchronously on every physics tick.

Default approach:

- commands perform durable mutations through explicit service/store boundaries;
- queries use read models appropriate to the feature;
- zones keep hot authoritative state locally;
- checkpoints persist bounded world state where recovery requirements need more than `game-server` graceful recovery;
- event sourcing remains optional.

Character identity must eventually be distinct from `game-server::PlayerId`, which is session/runtime-local.

## 8. Failure model

The architecture must explicitly test:

- host dies while owning a zone;
- network partition isolates an old host;
- control plane reassigns a zone with a higher epoch;
- stale host attempts a write/handoff afterward;
- source dies before destination accept;
- destination dies after accept but before source retirement;
- duplicate transfer messages arrive;
- a draining host receives new placement/admission.

Failover is not considered correct until these cases preserve single-writer authority and no player entity is silently duplicated or lost.

## 9. External foundations

Server hot-path dependencies stay small. Client/content systems are composed later:

- `3d-lab`: renderer/camera primitives;
- `input-bindings`: input semantics;
- `settings`: user settings;
- `asset-tooling`: authored asset processing and provenance;
- `social-service`: reusable social authority where applicable;
- `runtime-profiler`: performance evidence.

None of these becomes a source of zone gameplay truth.
