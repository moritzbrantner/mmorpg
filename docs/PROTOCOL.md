# Zone snapshot protocol

All multibyte fields are big-endian. Integers are unsigned unless marked `i32`/`i8`. Simulation positions use 100 units per metre; velocities use units per 30 Hz tick. Ticks and content revisions are 64-bit values; browser decoders retain them as `bigint`.

## Common snapshot header (18 bytes)

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Wire version: 2 |
| 1 | 1 | Scope: 1 canonical, 2 player-visible |
| 2 | 2 | Core schema version: 2 |
| 4 | 4 | Zone ID |
| 8 | 8 | Simulation tick |
| 16 | 2 | Player count, at most 512 |

## Player-visible scope

The header is followed by an 8-byte content revision and a 4-byte acknowledged command sequence for the addressed player. Each player record is 28 bytes: ID (`u32`), position (`3 × i32`), velocity (`3 × i32`). Total size is `30 + 28 × player_count`.

Player IDs are zone/session-local. These snapshots have no authority epoch field; the future online session/routing envelope must bind the stream to a grant and reset presentation on grant changes. An acknowledgement supports future prediction reconciliation, not permission to mutate authoritative state.

The shared fixture is `fixtures/protocol/player-snapshot-v2.hex`. Rust encoding and browser decoding both verify these exact bytes.

## Canonical scope

After the common header:

1. content revision (`u64`);
2. gravity (`3 × i32`);
3. static collider count (`u16`, at most 1024);
4. collider records: ID (`u32`), position (`3 × i32`), half extents (`3 × i32`), 28 bytes each;
5. player records: ID (`u32`), position (`3 × i32`), velocity (`3 × i32`), movement X and Z (`i8` each), last command sequence (`u32`), spawn slot (`u16`), 36 bytes each.

The content constructor validates and orders colliders. Core validates player uniqueness, spawn slots and movement intent during recovery. Default engine configuration and pinned physics behavior are part of the continuation contract.

Canonical data is for trusted replay/recovery and server-side verification. It must never be passed to the browser renderer or substituted for a player projection.

## Commands and migration

Movement commands retain wire version 1: `[version=1, tag=1, x:i8, z:i8]`, with each axis in `[-1, 1]`. The shared session runtime supplies player identity, connection epoch and command sequence separately. Core rejects zero/stale sequences.

Snapshot v1 is intentionally rejected. There is no implicit interpretation of old bytes as v2 and no bundled recovery migration. Protocol changes require updating version handling, this specification and both language contract tests together.
