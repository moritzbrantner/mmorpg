# Zone snapshot protocol

All multibyte fields are big-endian. Integers are unsigned unless marked `i8`/`i16`/`i32`. Simulation positions use 100 units per metre; velocities use units per 30 Hz tick. Ticks and content revisions are 64-bit values; browser decoders retain them as `bigint`.

A yaw or facing is a `u16`: 65 536 steps are one full turn. Yaw 0 faces +Z and increasing yaw turns toward +X, so the facing direction is `(sin yaw, cos yaw)` in XZ. A character's right is the direction of `yaw − 90°`; facing +Z, right is −X.

## Commands (wire version 2)

The shared session runtime supplies player identity, connection epoch and command sequence separately. Core rejects zero, stale and duplicate sequences without changing state.

| Command | Bytes | Layout |
| --- | ---: | --- |
| Move | 6 | `[version = 2, tag = 1, forward: i8, strafe: i8, facing: u16]` |
| Jump | 2 | `[version = 2, tag = 2]` |

- `Move` is held intent relative to `facing`. `forward` and `strafe` are each in `[-1, 1]`; positive strafe is the character's right. The server rotates the intent into one of eight headings (multiples of 45° relative to `facing`) through its integer trigonometry table. Horizontal speed is 21 units/tick (6.3 m/s) when `forward ≥ 0` and 13 units/tick when `forward < 0`. Zero intent stops horizontal movement; vertical velocity always stays with physics.
- `Jump` is edge-triggered. It is recorded as pending and evaluated during the next tick before physics steps: when a thin probe directly below the feet touches any body other than the character (ground, geometry or another unit), vertical velocity becomes 16 units/tick. The tick always consumes the pending jump, so a mid-air jump has no effect and is not buffered until landing.

Decoding is strict: exact lengths per tag, known tags only, `forward`/`strafe` in range, and only version 2. Version 1 (`SetMovement`) payloads are rejected.

The shared command fixture is `fixtures/protocol/commands-v2.hex`: one encoded command per line followed by its fields. `mmorpg-protocol` renders and verifies it, and the browser encoder (`web/src/command-wire.ts`) must produce the same bytes.

## Common snapshot prefix (16 bytes)

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Wire version: 3 |
| 1 | 1 | Scope: 1 canonical, 2 player-visible |
| 2 | 2 | Core schema version: 3 |
| 4 | 4 | Zone ID |
| 8 | 8 | Simulation tick |

## Player-visible scope

| Offset | Width | Field |
| --- | --- | --- |
| 16 | 8 | Content revision |
| 24 | 4 | Acknowledged command sequence of the addressed player |
| 28 | 4 | Viewer player ID: the entity that is "self" |
| 32 | 2 | Entity count, at most 64 (`MAX_VISIBLE_ENTITIES`) |
| 34 | 25 × count | Entity records |

Each entity record is 25 bytes:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Entity kind: 1 player. 2 (creature) and 3 (NPC) are reserved and currently rejected. |
| 1 | 4 | Entity ID, unique per kind |
| 5 | 12 | Position (`3 × i32`) |
| 17 | 6 | Velocity (`3 × i16`) |
| 23 | 2 | Facing (`u16` yaw) |

Total size is `34 + 25 × count`. Records are in relevance-priority order: the viewer first, then ascending `(squared XZ distance, kind, id)`. Velocity is presentation data: core saturates each component to the `i16` range when projecting, while canonical state keeps the exact physics velocity. The interest policy keeps players within the inclusive 4,500-unit (45 m) XZ radius, including the viewer, and caps the projection at the 64 highest-priority entities.

Player IDs are zone/session-local. These snapshots have no authority epoch field; the future online session/routing envelope must bind the stream to a grant and reset presentation on grant changes. An acknowledgement supports future prediction reconciliation, not permission to mutate authoritative state.

Decoders reject a wrong wire version, scope or schema, counts above capacity or not matching the payload length, unknown or reserved entity kinds, truncation and trailing bytes. The browser decoder additionally rejects duplicate `(kind, id)` identities. The native client rejects duplicates, a projection addressed to another viewer, and a projection without its own player.

The shared fixture is `fixtures/protocol/player-snapshot-v3.hex`. Rust encoding and browser decoding both verify these exact bytes.

## Canonical scope

After the common prefix:

1. player count (`u16`, at most 512);
2. content revision (`u64`);
3. gravity (`3 × i32`);
4. static collider count (`u16`, at most 1024);
5. collider records: ID (`u32`), position (`3 × i32`), half extents (`3 × i32`), 28 bytes each;
6. player records, 39 bytes each:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 4 | Player ID |
| 4 | 12 | Position (`3 × i32`) |
| 16 | 12 | Velocity (`3 × i32`) |
| 28 | 2 | Facing (`u16` yaw) |
| 30 | 1 | Forward intent (`i8`) |
| 31 | 1 | Strafe intent (`i8`) |
| 32 | 1 | Jump pending: 0 or 1 |
| 33 | 4 | Last command sequence |
| 37 | 2 | Spawn slot |

The content constructor validates and orders colliders. The decoder rejects a jump flag other than 0 or 1. Core validates player uniqueness, spawn slots and the forward/strafe range during recovery. Default engine configuration and pinned physics behavior are part of the continuation contract: recovering mid-run, mid-jump or with a pending jump reproduces the continuation exactly.

Canonical data is for trusted replay/recovery and server-side verification. It must never be passed to the browser renderer or substituted for a player projection.

## Versions and migration

Snapshot wire and core schema version 3 replace version 2 for both scopes, and command wire version 2 replaces version 1. Old bytes are never reinterpreted and there is no bundled migration:

- version 2 snapshots, canonical checkpoints and recovery bundles are rejected;
- `game-server` recovery bundles and replays captured before this change contain version 1 command payloads, which fail to decode, so a standalone host restarted with an old `MMORPG_RECOVERY_DIR` fails closed. Start from fresh development state or arrange an explicit migration.

Protocol changes require updating version handling, this specification, the shared fixture and both language contract tests together.
