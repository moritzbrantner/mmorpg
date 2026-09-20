# mmorpg

Server-authoritative MMORPG foundation designed to scale by distributing **zone ownership**, not by distributing one mutable physics simulation.

## Authority map

| Concern | Authority |
| --- | --- |
| Zone-local gameplay rules and interest policy | `mmorpg-core` |
| Collision and physical movement | `physics-engine` |
| Command/snapshot wire encoding | `mmorpg-protocol` |
| Session ticks, reconnects, replay/recovery, WebTransport | `game-server` |
| Zone placement, lease fencing, host routing, handoff metadata | `mmorpg-control-plane` |
| Adapter from a zone simulation into `game-server` | `mmorpg-game-server` |
| Rendering/client scene primitives | `3d-lab` (future client slice) |
| Runtime input semantics | `input-bindings` (future client slice) |
| User-facing settings | `settings` (future client slice) |
| Asset normalization/provenance | `asset-tooling` (future content slice) |
| Social systems | `social-service` where its existing authority fits |
| Runtime/performance evidence | `runtime-profiler` integration, without moving gameplay truth |

The MMO repository composes those foundations. It should not grow substitutes for them.

## Distributed shape

```text
                         control plane
                  zone directory / placement
                    leases + fencing epochs
                           /      \
                          /        \
                         v          v
                 zone host A     zone host B
                ┌────────────┐   ┌────────────┐
client ────────▶│ game-server│   │ game-server│◀──────── client
                │ MatchHost  │   │ MatchHost  │
                │ zone 10    │   │ zone 11    │
                └─────┬──────┘   └─────┬──────┘
                      v                v
                 mmorpg-core      mmorpg-core
                      |                |
                      v                v
                physics-engine   physics-engine
```

A zone is a single-writer authoritative simulation. A host may own many zones and the fleet may contain many hosts. Cross-zone movement transfers authority at a deterministic boundary using a unique transfer ID plus source/destination lease epochs. Two hosts must never concurrently own the same zone epoch.

This deliberately avoids shared mutable physics across machines. Horizontal scale comes from more zones, more zone hosts, bounded per-zone populations, player-scoped interest projections, and later evidence-driven zone splitting.

## Current foundation slice

The initial slice establishes:

- a Rust 1.98 workspace and reusable validation workflow;
- a deterministic zone simulation that uses the pinned `physics-engine`;
- stale-command rejection and bounded per-zone player capacity;
- canonical full-zone snapshots for replay/recovery;
- player-scoped interest snapshots for network publication;
- an explicit `game-server::GameSimulation` adapter;
- deterministic mapping from `ZoneId` to `game-server::MatchId`;
- a provider-neutral in-memory control-plane reference model with fenced, expiring zone leases;
- deterministic lease renewal/expiry and a restartable fencing-epoch floor contract;
- idempotent prepare/accept/commit state for cross-zone handoff metadata;
- a runnable multi-zone host with one WebTransport routing surface and separate operational status;
- architecture and roadmap documents that keep future persistence and orchestration choices replaceable.

The control-plane implementation in this slice is a **reference model**, not yet a production distributed consensus system. It exists to make ownership, epoch fencing and handoff idempotence executable before choosing storage or orchestration infrastructure.

## Scaling model

The first scaling unit is the zone:

1. one zone has one active authority lease;
2. one `game-server::MatchRuntime` drives that zone;
3. one process can host many zones through `MatchHost`;
4. more processes add more independent zone capacity;
5. a routing/control-plane layer tells clients and operators which host owns each zone;
6. cross-zone handoff moves player authority only after the destination accepts the transfer.

The initial `MAX_PLAYERS_PER_ZONE` is a safety bound, not a performance claim. Capacity changes require deterministic workload evidence. Large cities, raids, or crowded world events should eventually be handled by measured spatial/instance partitioning rather than silently increasing one process's bound.

## Persistence

Use lightweight CQRS/CQS at service boundaries: commands mutate authoritative durable state; queries read it. Do not introduce event sourcing by default. Zone hot loops must not synchronously depend on a distributed database. Durable character/world persistence and zone checkpoint storage are separate upcoming boundaries.

## Pinned foundations

- `physics-engine`: `c796ea382bdcb0276b9309e8a3cca34c8c28313b`
- `game-server`: `769de47005cc37891011fc76ae183c18b7c5e0ae`
- reusable validation workflow: `45042e56be120b438096e774027637cac0280075`

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
```

The committed workspace lockfile makes local and CI dependency resolution reproduce the same graph.

## Run a zone host

Provide a TLS certificate and key, then start one process hosting one or more zones:

```sh
MMORPG_ZONE_IDS=10,11 \
MMORPG_CERT_PEM=cert.pem \
MMORPG_KEY_PEM=key.pem \
MMORPG_RECOVERY_DIR=recovery/zone-host \
cargo run --locked -p mmorpg-game-server --bin mmorpg-zone-host
```

The host uses one WebTransport listener (port `4433` by default) and routes sessions under `/game/matches/zone-<id>`. Operational HTTP status is exposed separately on port `8080`:

- `/healthz` reports process liveness;
- `/readyz` reports whether at least one hosted zone can accept sessions;
- `/status` reports host capacity and per-zone readiness;
- `/matches/zone-<id>/readyz` and `/matches/zone-<id>/status` expose a single zone's operational projection.

When `MMORPG_RECOVERY_DIR` is set, graceful `SIGINT`/`SIGTERM` shutdown writes one atomic recovery bundle for the complete configured zone set. Restarting with the same zones restores simulation, command-sequence, and reconnect state, then consumes the old bundle. A missing zone, an extra zone, or corrupt recovery data fails startup closed instead of partially restoring a host. Durable crash recovery remains a separate persistence boundary.

These endpoints consume `game-server` host state and never mutate or redefine zone gameplay authority. Configuration is explicit through `MMORPG_ZONE_IDS`, `MMORPG_PORT`, `MMORPG_STATUS_PORT`, `MMORPG_CERT_PEM`, `MMORPG_KEY_PEM`, `MMORPG_RECOVERY_DIR`, `MMORPG_ROUTE_PREFIX`, `MMORPG_RECONNECT_GRACE_TICKS`, and `MMORPG_DRAIN_GRACE_MS`; recovery is opt-in, while invalid configured values fail startup instead of silently falling back.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [ROADMAP.md](ROADMAP.md).
