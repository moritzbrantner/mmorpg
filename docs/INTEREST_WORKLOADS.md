# Interest projection workloads

Core now uses an XZ spatial index to discover candidate players before applying the existing inclusive 2,000-unit distance rule. The live server already publishes through `ZoneSimulation::snapshot_for_player`, so it uses the index automatically. `project_for_player` returns the same snapshot with deterministic query counters for workload analysis.

Cells are one interest radius wide. Each query visits nine neighboring cells, then checks exact distance and returns players in ascending ID order. Negative coordinates use floor division; cell arithmetic and distance arithmetic cannot overflow at the supported integer coordinate limits. Height does not affect the existing visibility policy. This remains relevance filtering, not line of sight or stealth authorization.

The index stores only player IDs, with at most one entry per player and one bucket per occupied cell. Admission and removal update it immediately. After a successful physics step, core rebuilds it once from authoritative positions, including collision corrections. Recovery reconstructs it from canonical state. Index contents never enter checkpoints, replay hashes, or the wire format; schema and wire versions remain 2.

## Reproduce

```sh
cargo test -p mmorpg-core --test interest --locked
cargo run -p mmorpg-protocol --example interest_workload --locked
```

The example emits three JSON lines under `mmorpg.interest-workload/v1`. Each workload restores 512 stationary players, advances one real physics tick, then projects once for every player. It also encodes an independent exhaustive projection for every recipient and fails if any output byte differs. No public network, clock, randomness, or GPU is involved.

The baseline is the exhaustive XZ rule from repository revision `835e2be8e648db5098a2fca0701b31f0998fcbe3`: 512 × 512 = 262,144 distance tests per complete publication sweep. The comparison uses identical authoritative state and encoding. Smaller distance-test counts are preferable; snapshot sizes must remain identical for equivalent visibility.

| Workload | Layout | Indexed distance tests | Baseline distance tests | Total snapshot payload bytes | Largest payload bytes |
| --- | --- | ---: | ---: | ---: | ---: |
| sparse-travel-512 | Players 6,000 units apart along X | 512 | 262,144 | 29,696 | 58 |
| dense-hub-512 | 23-column grid at 64-unit spacing; everyone in range | 262,144 | 262,144 | 7,355,392 | 14,366 |
| outpost-spawn-512 | Shared outpost, default 32-column spawn grid | 190,464 | 262,144 | 2,885,696 | 7,982 |

Every workload also performs 4,608 bucket lookups per publication sweep and rebuilds 512 index entries per tick. Rebuilds, tree operations, candidate sorting, allocations, encoding and physics have costs not captured by distance-test counts. These measurements prove reduced discovery work in sparse zones, not a 512× runtime speedup or a supported player throughput. Fully dense visibility still requires quadratic output work. Physics throughput needs separate profiling in the owning engine.

Payload bytes above count only the MMO visible snapshot, excluding the shared session frame, QUIC, TLS and UDP overhead. Dense visibility is intentionally complete: the index must never drop nearby players to fit a packet. The current whole-datagram transport still needs bounded large-snapshot replication in `game-server`; these results do not claim 512 networked clients are supported.

## Regression evidence

The core gate compares indexed and exhaustive results across inclusive radius edges, diagonal edges, vertical separation, negative cells, extreme coordinates, a fixed scattered layout, movement across cells, collision resolution, failed physics steps, admission, removal and checkpoint recovery. It also asserts exact sparse/dense work counts without wall-clock thresholds. Stable ordering, acknowledgements, content revisions and read-only query behavior are checked alongside visibility.

Recorded measurement environment: Rust 1.98.0, `Cargo.lock` SHA-256 `324fe3ee2707eb2b819239838ce0fe95783b8f5cdf2470e7604552d020209d09`, core schema 2, wire version 2, and outpost revision 1. The counts are architecture-independent integer operations and byte lengths. Shared conventions resolved to sourceRevision `e6acb5310afaf15c0cba24f87108f5f4ad1bedc3`; this is evidence provenance, not a policy pin. No dependency changes were needed.
