# Interest projection workloads

Core uses an XZ spatial index to discover candidate players before applying the inclusive 4,500-unit (45 m) distance rule and the deterministic priority cap. The live server publishes through `ZoneSimulation::snapshot_for_player`, so it uses the index automatically. `project_for_player` returns the same snapshot with deterministic query counters for workload analysis.

Cells are one interest radius wide. Each query visits nine neighboring cells and checks exact distance. The relevant players are then ordered viewer first, then by ascending `(squared XZ distance, kind, id)`, and the first `MAX_VISIBLE_ENTITIES` (64) are kept: the relevance cap sized so the largest projection fits one datagram ([PROTOCOL.md](PROTOCOL.md#datagram-byte-budget)). Negative coordinates use floor division; cell arithmetic and distance arithmetic cannot overflow at the supported integer coordinate limits. Height does not affect the visibility policy. This remains relevance filtering, not line of sight or stealth authorization.

The derived index stores player IDs in occupied cells and a player-to-cell membership map. Admission and removal update it immediately. After a successful physics step, core reads authoritative physics positions and changes bucket memberships only when a player's cell changes, including collision corrections. Recovery reconstructs it from canonical state. Index contents and work counters never enter checkpoints, replay hashes, or the wire format.

## Reproduce

```sh
cargo test -p mmorpg-core --test interest --locked
cargo run -p mmorpg-protocol --example interest_workload --locked
```

The example emits three JSON lines under `mmorpg.interest-workload/v3` (v3 adds `relevant_records`, the in-radius count before the cap). Each workload restores 512 stationary players, advances one real physics tick, then projects once for every player. It also encodes an independent exhaustive projection (the same radius, order and cap, without buckets) for every recipient and fails if any output byte differs. Maintenance counters measure only the tick, excluding recovery construction; query counters measure the publication sweep. No public network, clock, randomness, or GPU is involved.

The baseline is the exhaustive XZ rule: 512 × 512 = 262,144 distance tests per complete publication sweep. The comparison uses identical authoritative state and encoding. Smaller distance-test counts are preferable; snapshot sizes must remain identical for equivalent visibility.

| Workload | Layout | Indexed distance tests | Baseline distance tests | Relevant records | Published records | Total snapshot payload bytes | Largest payload bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| sparse-grid-512 | 23-column grid, 28 m apart, spanning ±308 m | 10,626 | 262,144 | 4,338 | 4,338 | 86,816 | 178 |
| dense-hub-512 | 23-column grid at 64-unit spacing; everyone in range | 262,144 | 262,144 | 262,144 | 32,768 | 541,696 | 1,058 |
| vale-spawn-512 | Greyhaven Vale, full 32 × 16 hub spawn plaza (1 m apart) | 262,144 | 262,144 | 262,144 | 32,768 | 541,696 | 1,058 |

Every workload performs 4,608 query bucket visits per publication sweep. The stationary sparse workload requires 512 player position inspections but zero full index rebuilds, bucket inserts, removes, or moves during the tick. The core regression asserts zero membership writes for same-cell movement and two moves for two crossing players, including negative cell coordinates. Tree lookups, candidate sorting, allocations, encoding and physics have costs not captured by distance-test counts. These measurements prove reduced discovery and maintenance work, not a supported player throughput. Dense crowds still cost quadratic candidate discovery; the cap bounds output, not discovery. Physics throughput needs separate profiling in the owning engine.

Payload bytes above count only the MMO visible snapshot (wire version 4: a 34-byte header plus 16 bytes per published entity), excluding the shared session frame, QUIC, TLS and UDP overhead. Wire version 3 used 25-byte records and no cap: the former dense workloads published 512 records and up to 12,834 bytes per projection, which the pinned whole-datagram transport could not deliver. With the cap, the largest projection is 1,058 bytes, inside the 1,077-byte budget. Dense relevance beyond the cap is omitted by deterministic priority, never truncated by transport. The canonical payload of the vale workload (26,910 bytes) includes its embedded content revision (colliders and spawn grid) and is not sent to clients.

## Regression evidence

The core gate compares indexed and exhaustive results across inclusive radius edges, diagonal edges, vertical separation, negative cells, extreme coordinates, a fixed scattered layout, dense layouts beyond the cap, movement across cells, collision resolution, failed physics steps, admission, removal and checkpoint recovery. The protocol gate runs the shared exhaustive reference and indexed projection through deterministic multi-tick sequences and compares complete snapshots and encoded bytes after each transition; positions outside the compact wire range fail closed identically, and players running on content without walls stay encodable because the world limits stop them. It also asserts exact sparse/dense query work and membership maintenance counts without wall-clock thresholds. Stable ordering, acknowledgements, content revisions and read-only query behavior are checked alongside visibility.

Recorded measurement environment: Rust 1.98.1, `Cargo.lock` SHA-256 `36adf21ccc47ab0a19e8fd6e8792cb1fbd0605d9ac70070d656b6fdec10a0157`, core schema 4, wire version 4, and Greyhaven Vale content revision 2. The counts are architecture-independent integer operations and byte lengths.
