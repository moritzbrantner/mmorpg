# Zone snapshot protocol

All multibyte fields are big-endian. Integers are unsigned unless marked `i8`/`i16`/`i32`. Simulation positions use 100 units per metre; velocities use units per 30 Hz tick. Ticks and content revisions are 64-bit values; browser decoders retain them as `bigint`.

A yaw or facing is a `u16`: 65 536 steps are one full turn. Yaw 0 faces +Z and increasing yaw turns toward +X, so the facing direction is `(sin yaw, cos yaw)` in XZ. A character's right is the direction of `yaw − 90°`; facing +Z, right is −X.

## Commands (wire version 2)

The shared session runtime supplies player identity, connection epoch and command sequence separately. Core rejects zero, stale and duplicate sequences without changing state.

| Command | Bytes | Layout |
| --- | ---: | --- |
| Move | 6 | `[version = 2, tag = 1, forward: i8, strafe: i8, facing: u16]` |
| Jump | 2 | `[version = 2, tag = 2]` |
| SelectTarget | 7 | `[version = 2, tag = 3, kind: u8, id: u32]` |
| StartAttack | 2 | `[version = 2, tag = 4]` |
| StopAttack | 2 | `[version = 2, tag = 5]` |
| ReleaseSpirit | 2 | `[version = 2, tag = 6]` |

Tags 3–6 were added for targeting and combat. They are additive: tags 1 and 2 are unchanged, so the command wire version stays 2.

- `Move` is held intent relative to `facing`. `forward` and `strafe` are each in `[-1, 1]`; positive strafe is the character's right. The server rotates the intent into one of eight headings (multiples of 45° relative to `facing`) through its integer trigonometry table. Horizontal speed is 21 units/tick (6.3 m/s) when `forward ≥ 0` and 13 units/tick when `forward < 0`. Zero intent stops horizontal movement; vertical velocity always stays with physics.
- `Jump` is edge-triggered. It is recorded as pending and evaluated during the next tick before physics steps: when a thin probe directly below the feet touches any body other than the character (ground, geometry or another unit), vertical velocity becomes 16 units/tick. The tick always consumes the pending jump, so a mid-air jump has no effect and is not buffered until landing.
- `SelectTarget` carries an [entity reference](#entity-references): kind 1 player, 2 creature or 3 NPC with its ID, or kind 0 with ID 0 to clear the selection. `StartAttack` starts auto-attacking the selected target, `StopAttack` stops it, and `ReleaseSpirit` returns a dead player to the graveyard with half health.

The four discrete intents are queued in sequence order (at most 16 per player between ticks) and resolved during the next tick in `(player id, sequence)` order, never inside `apply_command`. A well-formed intent that is not allowed right now, such as attacking without a target or out of range, or selecting a unit that is not visible, is accepted and answered with an `Error` [event](#events); it never closes the session. An intent that finds the queue full is accepted the same way: it consumes its sequence, is dropped, and the next tick answers with one `too many intents` error. Only malformed payloads and stale or duplicate sequences fail.

Decoding is strict: exact lengths per tag, known tags only, `forward`/`strafe` in range, known entity kinds, ID 0 for the absent reference, and only version 2. Version 1 (`SetMovement`) payloads are rejected.

The shared command fixture is `fixtures/protocol/commands-v2.hex`: one encoded command per line followed by its fields, covering every tag. `mmorpg-protocol` renders and verifies it, and the browser encoder (`web/src/command-wire.ts`) must produce the same bytes.

## Entity references

An entity reference is 5 bytes: kind (`u8`) then ID (`u32`). Kind 1 is a player (session-local player ID), 2 a creature (its spawn ID in zone content), 3 an NPC (its content ID). Kind 0 means no unit and requires ID 0. Ordering units by `(kind, id)` is the `EntityRef` order core uses for every ordered pass.

## Common snapshot prefix (16 bytes)

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Wire version: 5 |
| 1 | 1 | Scope: 1 canonical, 2 player-visible |
| 2 | 2 | Core schema version: 5 |
| 4 | 4 | Zone ID |
| 8 | 8 | Simulation tick |

## Player-visible scope

Sections follow in fixed order: header, self, target, events, entities.

| Offset | Width | Field |
| --- | --- | --- |
| 16 | 8 | Content revision |
| 24 | 4 | Acknowledged command sequence of the addressed player |
| 28 | 4 | Viewer player ID: the entity that is "self" |
| 32 | 4 | Self: health |
| 36 | 4 | Self: maximum health |
| 40 | 1 | Self: level (at least 1) |
| 41 | 1 | Self flags: bit 0 dead, bit 1 in combat, bit 2 auto-attacking; other bits 0 |
| 42 | 5 | Self: target (entity reference, kind 0 for none) |
| 47 | 5 | Target: the target's own target (entity reference) |
| 52 | 1 | Event count, at most 16 |
| 53 | 14 × events | Event records |
| … | 2 | Entity count |
| … | 21 × count | Entity records |

The self section is the viewer's exact state: `dead` holds exactly when health is 0, and health never exceeds its maximum. The target section is the target-of-target: a player's selection or an engaged creature's threat leader, so entity records need no per-entity target. The fixed part is 55 bytes; a projection is `55 + 14 × events + 21 × entities` bytes.

### Events

The events are the viewer's feedback from the tick the snapshot describes (see [ARCHITECTURE.md](ARCHITECTURE.md#units-combat-and-creature-ai)). Each record is 14 bytes:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Kind: 1 damage dealt, 2 damage taken, 3 miss, 4 died, 5 evade, 6 error |
| 1 | 1 | Flags: bit 0 critical, allowed only for damage; other bits 0 |
| 2 | 5 | Source (entity reference) |
| 7 | 5 | Target (entity reference) |
| 12 | 2 | Amount |

- Damage dealt and damage taken carry both units and the damage in `amount`.
- Miss and evade carry both units and amount 0: a missed swing between the viewer and a unit, or the viewer's swing ignored by an evading creature.
- Died carries the dead unit as target, its killer as source (or none) and amount 0.
- Error carries no source, the unit the refused intent concerned as target (or none) and the error code in `amount`: 1 no target, 2 out of range, 3 target dead, 4 not attackable, 5 you are dead, 6 not dead, 7 invalid target, 8 too many intents (a full queue dropped an intent).

Events are cosmetic: a lost datagram may lose them. Health and every other durable fact is repeated in every projection.

### Entity records

Each entity record is 21 bytes:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Entity kind: 1 player, 2 creature, 3 NPC |
| 1 | 4 | Entity ID, unique per kind |
| 5 | 2 | Appearance: creature template ID, NPC ID, 0 for players until classes land |
| 7 | 6 | Position (`3 × i16`, absolute units) |
| 13 | 3 | Velocity (`3 × i8`, units per tick) |
| 16 | 2 | Facing (`u16` yaw) |
| 18 | 1 | Level |
| 19 | 1 | Health percent, 0–100, rounded up so a living unit never shows 0 |
| 20 | 1 | Flags: bit 0 dead, bit 1 in combat, bit 2 hostile, bit 3 attackable, bit 4 tapped by another player, bit 5 evading, bit 6 targets the viewer; bit 7 is 0 |

Records are in relevance-priority order: the viewer first, then the viewer's current target when it is a unit or corpse within the radius, then ascending `(squared XZ distance, kind, id)`. The interest policy keeps players, living creatures, corpses and NPCs within the inclusive 4,500-unit (45 m) XZ radius and caps relevance at the 64 (`MAX_VISIBLE_ENTITIES`) highest-priority units; the byte budget below then decides how many of those are written. Creature and NPC names come from content by appearance ID ([`mmorpg-wasm`'s catalog](../crates/mmorpg-wasm/src/catalog.rs) for the browser). A corpse keeps its record, flagged dead with health 0, until it despawns.

Positions are absolute `i16` units. Zone content keeps every collider bound and every spawned body within ±32,000 units (`MAX_CONTENT_COORDINATE_UNITS`). The simulation closes that cube with six fixed world-limit bodies that are not content, so no player admitted at a spawn slot can leave it, whatever the content's walls or gravity; content boundary walls, such as Greyhaven Vale's, are the gameplay boundary inside it. Recovery restores captured positions unchanged. The encoder still fails closed rather than clamping a position outside the `i16` range. Canonical state keeps full `i32` positions. Velocity is presentation data: core saturates each component to the `i8` range when projecting, while canonical state keeps the exact physics velocity (run speed is 21 and jump velocity 16 units/tick, well inside that range).

### Datagram byte budget

The pinned `game-server` sends a session snapshot frame unchanged when it fits the connection's current WebTransport datagram size. Otherwise it sends bounded, tick-keyed fragments; the native client reassembles and verifies the session frame before decoding this v5 payload. A lost fragment loses that snapshot, and newer complete ticks supersede older incomplete ones. The reassembler is reset on reconnect. This transport behavior does not change the MMO projection policy: its current budget derives from the smallest negotiated datagram size:

| Term | Bytes | Source |
| --- | ---: | --- |
| Measured floor (`MEASURED_MIN_DATAGRAM_BYTES`) | 1,161 | QUIC's 1,200-byte initial MTU before path MTU discovery, observed by both peers over the pinned wtransport/quinn stack |
| `game-server` snapshot frame header | − 20 | `game_server::SNAPSHOT_HEADER_BYTES` |
| Safety margin | − 64 | Headroom for unmeasured transport overhead |
| **`MAX_PLAYER_PROJECTION_BYTES`** | **1,077** | Largest player projection payload |

On loopback, path MTU discovery raises the host's value to 1,287 bytes before admission and the client's to 1,350–1,413 bytes, but on a real path the host can capture its limit before discovery completes, so the budget uses the floor. `mmorpg-client`'s `connected_world` tests log the negotiated size, pin the floor with discovery disabled on both peers, and deliver a crowded projection packed to the full 48-record budget through a real host.

Encoding is budget-driven in fixed section priority: `pack_snapshot` writes the header, the self and target sections and this tick's events, then entities in priority order until the next record would exceed the budget, and reports how many it packed. Hosts publish with it. Without events, `(1,077 − 55) / 21 = 48` records fit (`MAX_WIRE_ENTITIES`), a 1,063-byte projection; with a full event section, 38 do. The viewer and its target lead the entities and always fit: `55 + 16 × 14 + 2 × 21 = 321` bytes, which a compile-time assertion checks. Relevant units beyond the budget are omitted by priority, never truncated by transport. `encode_snapshot`, used by fixtures and tests, fails closed instead of omitting any entity. A test with extreme field values (maximum IDs, tick and revision, `i16`/`i8` extremes, a full event section and the whole relevance cap) proves the packing fits one datagram. Future payload growth requires a deliberate MMO budget and wire change; transport fragmentation alone does not raise the budget. Later sections (character sheet, loot) take their place before the entities.

Player IDs are zone/session-local. These snapshots have no authority epoch field; the future online session/routing envelope must bind the stream to a grant and reset presentation on grant changes. An acknowledgement supports future prediction reconciliation, not permission to mutate authoritative state.

Decoders reject payloads above the byte budget, a wrong wire version, scope or schema, inconsistent self state (level 0, health above its maximum, a dead flag that disagrees with zero health), more than 16 events or 48 entities, unknown entity kinds, event kinds or error codes, event flags or fields that do not fit their kind, an absent reference with a non-zero ID, reserved flag bits, a health percent above 100, counts not matching the payload length, a first record that is not the viewer, truncation and trailing bytes. The browser decoder additionally rejects duplicate `(kind, id)` identities. The native client rejects duplicates, a projection addressed to another viewer, and a projection without its own player.

The shared fixture is `fixtures/protocol/player-snapshot-v5.hex`: a viewer fighting a wolf next to an NPC and a corpse tapped by another player, with one event of every kind. Rust encoding and browser decoding both verify these exact bytes.

## Canonical scope

After the common prefix:

1. content revision (`u64`);
2. content fingerprint (`u64`);
3. zone RNG state (`u64`);
4. player count (`u16`, at most 512), then player records;
5. creature count (`u16`, at most 1,024), then creature records in creature-ID order.

Content is referenced, not embedded: the revision and the fingerprint (FNV-1a 64 over a canonical encoding of every content table, colliders included) identify the exact `ZoneContent`, and `ZoneSimulation::from_snapshot` fails closed unless the supplied content has both (the content-addressed checkpoint rule in [ARCHITECTURE.md](ARCHITECTURE.md#snapshots-and-compatibility)). Static colliders, NPCs and world-limit bodies come from that content during recovery.

A player record starts with the 39 bytes of movement state and continues with its unit state:

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
| 39 | 1 | Level |
| 40 | 4 | Health (0 means dead) |
| 44 | 5 | Target (entity reference) |
| 49 | 1 | Auto-attacking: 0 or 1 |
| 50 | 2 | Swing timer (ticks until the next swing) |
| 52 | 2 | Combat timer (ticks left in combat after the last blow) |
| 54 | 2 | Calm ticks (out-of-combat ticks driving regeneration) |
| 56 | 2 | Error cooldown (ticks until the next out-of-range error) |
| 58 | 1 | Pending intent count, at most 16 |
| 59 | 6 × intents | Intent code (1 select target, 2 start attack, 3 stop attack, 4 release spirit) and entity reference, which is none except for select target |
| … | 1 | Intents dropped: 0 or 1; a full queue dropped a later intent, which the next tick reports |
| … | 1 | Event count, at most 16 |
| … | 14 × events | This tick's events, as in the player-visible scope |

A creature record:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 4 | Creature ID (its spawn ID) |
| 4 | 1 | Level |
| 5 | 4 | Health |
| 9 | 2 | Facing |
| 11 | 12 | Position (`3 × i32`): the body, or the corpse |
| 23 | 12 | Velocity (`3 × i32`) |
| 35 | 1 | Life: 1 alive, 2 corpse, 3 despawned |
| 36 | 8 | Death tick (0 while alive) |
| 44 | 1 | AI: 1 idle, 2 engaged, 3 evading |
| 45 | 2 | AI timer: idle wait or walk ticks, evade ticks so far; 0 when engaged |
| 47 | 1 | Wander destination present: 0 or 1 (idle only) |
| 48 | 8 | Wander destination (`2 × i32`, XZ), zero when absent |
| 56 | 1 | Threat entry count, at most 64 |
| 57 | 9 × entries | Threat entries in entity order: entity reference and threat (`u32`) |
| … | 2 | Swing timer |
| … | 2 | Combat timer |
| … | 1 | Tapped: 0 or 1 |
| … | 4 | Tapping player ID, 0 when untapped |

The decoder rejects booleans other than 0 or 1, unknown codes, an alive creature with a death tick, AI fields that do not fit their state, an absent threat unit, an untapped creature with a tapper, excessive counts, truncation and trailing bytes. Core then validates the state against the content during recovery: player uniqueness, spawn slots, movement range, level and health bounds, queue sizes, that only a full intent queue has dropped intents, that dead players do not auto-attack, that creature records match the content's spawns one-to-one in ID order with levels and health inside their template and state consistent with their life cycle (corpses and despawned creatures rest; only engaged creatures have threat, and threat tables hold only living players), and that player targets exist. Default engine configuration and pinned physics behavior are part of the continuation contract: recovering mid-run, mid-jump, with a pending jump, mid-chase, mid-swing, after a death or during an evade reproduces the continuation exactly.

Canonical data is for trusted replay/recovery and server-side verification. It must never be passed to the browser renderer or substituted for a player projection.

## Versions and migration

Snapshot wire and core schema version 5 replace version 4 for both scopes: player projections gained the self, target and event sections and 21-byte entity records for players, creatures and NPCs; canonical snapshots reference content by revision and fingerprint instead of embedding it and carry unit, creature, RNG and event state. Command wire version 2 gained tags 3–6 additively. Old bytes are never reinterpreted and there is no bundled migration:

- version 4 snapshots, canonical checkpoints and recovery bundles are rejected;
- a standalone host restarted with an old `MMORPG_RECOVERY_DIR` fails closed. Start from fresh development state or arrange an explicit migration.

Protocol changes require updating version handling, this specification, the shared fixture and both language contract tests together.
