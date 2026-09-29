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
| 0 | 1 | Wire version: 4 |
| 1 | 1 | Scope: 1 canonical, 2 player-visible |
| 2 | 2 | Core schema version: 4 |
| 4 | 4 | Zone ID |
| 8 | 8 | Simulation tick |

## Player-visible scope

| Offset | Width | Field |
| --- | --- | --- |
| 16 | 8 | Content revision |
| 24 | 4 | Acknowledged command sequence of the addressed player |
| 28 | 4 | Viewer player ID: the entity that is "self" |
| 32 | 2 | Entity count |
| 34 | 16 × count | Entity records |

Each entity record is 16 bytes:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Entity kind: 1 player. 2 (creature) and 3 (NPC) are reserved and currently rejected. |
| 1 | 4 | Entity ID, unique per kind |
| 5 | 6 | Position (`3 × i16`, absolute units) |
| 11 | 3 | Velocity (`3 × i8`, units per tick) |
| 14 | 2 | Facing (`u16` yaw) |

Total size is `34 + 16 × count`. Records are in relevance-priority order: the viewer first, then ascending `(squared XZ distance, kind, id)`. The interest policy keeps players within the inclusive 4,500-unit (45 m) XZ radius, including the viewer, and caps the projection at the 64 (`MAX_VISIBLE_ENTITIES`) highest-priority entities.

Positions are absolute `i16` units. Zone content keeps every collider bound and every spawned body within ±32,000 units (`MAX_CONTENT_COORDINATE_UNITS`). The simulation closes that cube with six fixed world-limit bodies that are not content, so no player admitted at a spawn slot can leave it, whatever the content's walls or gravity; content boundary walls, such as Greyhaven Vale's, are the gameplay boundary inside it. Recovery restores captured positions unchanged. The encoder still fails closed rather than clamping a position outside the `i16` range. Canonical state keeps full `i32` positions. Velocity is presentation data: core saturates each component to the `i8` range when projecting, while canonical state keeps the exact physics velocity (run speed is 21 and jump velocity 16 units/tick, well inside that range).

### Datagram byte budget

The pinned `game-server` sends a session snapshot frame unchanged when it fits the connection's current WebTransport datagram size. Otherwise it sends bounded, tick-keyed fragments; the native client reassembles and verifies the session frame before decoding this v4 payload. A lost fragment loses that snapshot, and newer complete ticks supersede older incomplete ones. The reassembler is reset on reconnect. This transport behavior does not change the MMO projection policy: its current budget derives from the smallest negotiated datagram size:

| Term | Bytes | Source |
| --- | ---: | --- |
| Measured floor (`MEASURED_MIN_DATAGRAM_BYTES`) | 1,161 | QUIC's 1,200-byte initial MTU before path MTU discovery, observed by both peers over the pinned wtransport/quinn stack |
| `game-server` snapshot frame header | − 20 | `game_server::SNAPSHOT_HEADER_BYTES` |
| Safety margin | − 64 | Headroom for unmeasured transport overhead |
| **`MAX_PLAYER_PROJECTION_BYTES`** | **1,077** | Largest player projection payload |

On loopback, path MTU discovery raises the host's value to 1,287 bytes before admission and the client's to 1,350–1,413 bytes, but on a real path the host can capture its limit before discovery completes, so the budget uses the floor. `mmorpg-client`'s `connected_world` tests log the negotiated size, pin the floor with discovery disabled on both peers, and deliver a projection at the relevance cap through a real host.

The largest projection is `34 + 64 × 16 = 1,058` bytes, 19 bytes under the budget; at most `(1,077 − 34) / 16 = 65` records fit. A compile-time assertion and a test with extreme field values (maximum IDs, tick and revision; `i16`/`i8` extremes) prove the cap fits.

Encoding is budget-driven: `pack_snapshot` writes the header, then entities in priority order until the next record would exceed the budget, and reports how many it packed. The viewer leads every projection and always fits; later steps write higher-priority sections (self state, the viewer's current target) before the remaining entities. `encode_snapshot`, which hosts use, fails closed instead of omitting any entity, so an overflow is an error rather than silent truncation. Future payload growth requires a deliberate MMO budget and wire change; transport fragmentation alone does not raise the 64-entity cap.

Player IDs are zone/session-local. These snapshots have no authority epoch field; the future online session/routing envelope must bind the stream to a grant and reset presentation on grant changes. An acknowledgement supports future prediction reconciliation, not permission to mutate authoritative state.

Decoders reject payloads above the byte budget, a wrong wire version, scope or schema, counts not matching the payload length, unknown or reserved entity kinds, a first record that is not the viewer, truncation and trailing bytes. The browser decoder additionally rejects duplicate `(kind, id)` identities. The native client rejects duplicates, a projection addressed to another viewer, and a projection without its own player.

The shared fixture is `fixtures/protocol/player-snapshot-v4.hex`. Rust encoding and browser decoding both verify these exact bytes.

## Canonical scope

After the common prefix:

1. player count (`u16`, at most 512);
2. content revision (`u64`);
3. gravity (`3 × i32`);
4. spawn grid: slot-0 feet origin X and Z (`2 × i32`), columns (`u16`), spacing (`i32`), 14 bytes;
5. static collider count (`u16`, at most 1024);
6. collider records: ID (`u32`), position (`3 × i32`), half extents (`3 × i32`), 28 bytes each;
7. player records, 39 bytes each:

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

The content constructor validates and orders colliders, keeps every collider bound and spawned body within ±32,000 units, and proves all 512 spawn slots are clear of colliders. World-limit bodies are derived from that range during construction and recovery; they are not serialized. The decoder rejects a jump flag other than 0 or 1. Core validates player uniqueness, spawn slots and the forward/strafe range during recovery. Default engine configuration and pinned physics behavior are part of the continuation contract: recovering mid-run, mid-jump or with a pending jump reproduces the continuation exactly.

Canonical data is for trusted replay/recovery and server-side verification. It must never be passed to the browser renderer or substituted for a player projection.

## Versions and migration

Snapshot wire and core schema version 4 replace version 3 for both scopes: player records became compact (`i16` positions, `i8` velocity) and priority-ordered, and canonical content carries the spawn grid. Command wire version 2 is unchanged. Old bytes are never reinterpreted and there is no bundled migration:

- version 3 snapshots, canonical checkpoints and recovery bundles are rejected;
- a standalone host restarted with an old `MMORPG_RECOVERY_DIR` fails closed. Start from fresh development state or arrange an explicit migration.

Protocol changes require updating version handling, this specification, the shared fixture and both language contract tests together.
