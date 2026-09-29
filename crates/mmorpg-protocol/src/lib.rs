#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, EntityKind, EntitySnapshot,
    MAX_PLAYERS_PER_ZONE, MAX_STATIC_COLLIDERS, MAX_VISIBLE_ENTITIES, SNAPSHOT_SCHEMA_VERSION,
    SpawnGrid, StaticCollider, ZoneCommand, ZoneDefinition, ZoneId, ZoneSnapshot,
};

pub const COMMAND_WIRE_VERSION: u8 = 2;
pub const SNAPSHOT_WIRE_VERSION: u8 = 4;

/// Smallest WebTransport datagram payload measured over the pinned stack:
/// QUIC's 1,200-byte initial MTU before path MTU discovery, observed as 1,161
/// bytes by both peers (`mmorpg-client` `connected_world` tests). This remains
/// the current projection-policy budget even though the transport can fragment
/// larger session frames.
pub const MEASURED_MIN_DATAGRAM_BYTES: usize = 1_161;
/// `game-server` session frame header in front of every snapshot payload.
pub const SESSION_SNAPSHOT_HEADER_BYTES: usize = 20;
/// Headroom for transport overhead the measurement does not cover.
pub const DATAGRAM_SAFETY_MARGIN_BYTES: usize = 64;
/// Largest player projection payload under the current MMO relevance policy.
/// Transport fragmentation permits future sections to grow beyond this budget
/// once the projection policy and both decoders change together.
pub const MAX_PLAYER_PROJECTION_BYTES: usize =
    MEASURED_MIN_DATAGRAM_BYTES - SESSION_SNAPSHOT_HEADER_BYTES - DATAGRAM_SAFETY_MARGIN_BYTES;

const MOVE_TAG: u8 = 1;
const JUMP_TAG: u8 = 2;
const MOVE_COMMAND_BYTES: usize = 6;
const JUMP_COMMAND_BYTES: usize = 2;
const CANONICAL_SNAPSHOT_SCOPE: u8 = 1;
const PLAYER_SNAPSHOT_SCOPE: u8 = 2;
/// Wire code of `EntityKind::Player`; 2 (creature) and 3 (NPC) are reserved.
const PLAYER_ENTITY_KIND: u8 = 1;
/// Wire version, scope, schema, zone ID and tick start every snapshot.
const COMMON_HEADER_BYTES: usize = 16;
pub const PLAYER_SNAPSHOT_HEADER_BYTES: usize = 34;
/// Kind, ID, `3 × i16` position, `3 × i8` velocity and `u16` facing.
pub const ENTITY_RECORD_BYTES: usize = 16;
/// Records that fit the budget after the header: (1,077 − 34) / 16 = 65.
const MAX_WIRE_ENTITIES: usize =
    (MAX_PLAYER_PROJECTION_BYTES - PLAYER_SNAPSHOT_HEADER_BYTES) / ENTITY_RECORD_BYTES;
const CANONICAL_PLAYER_RECORD_BYTES: usize = 39;

// A projection at core's relevance cap always fits one datagram:
// 34 + 64 × 16 = 1,058 ≤ 1,077 bytes.
const _: () = assert!(
    PLAYER_SNAPSHOT_HEADER_BYTES + MAX_VISIBLE_ENTITIES * ENTITY_RECORD_BYTES
        <= MAX_PLAYER_PROJECTION_BYTES
);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolError {
    message: String,
}

impl ProtocolError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ProtocolError {}

#[must_use]
pub fn encode_command(command: ZoneCommand) -> Vec<u8> {
    match command {
        ZoneCommand::Move {
            forward,
            strafe,
            facing,
        } => {
            let mut payload = Vec::with_capacity(MOVE_COMMAND_BYTES);
            payload.extend_from_slice(&[COMMAND_WIRE_VERSION, MOVE_TAG]);
            payload.extend_from_slice(&forward.to_be_bytes());
            payload.extend_from_slice(&strafe.to_be_bytes());
            payload.extend_from_slice(&facing.to_be_bytes());
            payload
        }
        ZoneCommand::Jump => vec![COMMAND_WIRE_VERSION, JUMP_TAG],
    }
}

pub fn decode_command(payload: &[u8]) -> Result<ZoneCommand, ProtocolError> {
    let [version, tag, ..] = payload else {
        return Err(ProtocolError::new("command payload is truncated"));
    };
    if *version != COMMAND_WIRE_VERSION {
        return Err(ProtocolError::new("unsupported command wire version"));
    }
    match *tag {
        MOVE_TAG => {
            let [_, _, forward, strafe, facing_high, facing_low] = payload else {
                return Err(ProtocolError::new(
                    "move command payload must be exactly 6 bytes",
                ));
            };
            let forward = i8::from_be_bytes([*forward]);
            let strafe = i8::from_be_bytes([*strafe]);
            if !(-1..=1).contains(&forward) || !(-1..=1).contains(&strafe) {
                return Err(ProtocolError::new(
                    "movement components must be between -1 and 1",
                ));
            }
            Ok(ZoneCommand::Move {
                forward,
                strafe,
                facing: u16::from_be_bytes([*facing_high, *facing_low]),
            })
        }
        JUMP_TAG if payload.len() == JUMP_COMMAND_BYTES => Ok(ZoneCommand::Jump),
        JUMP_TAG => Err(ProtocolError::new(
            "jump command payload must be exactly 2 bytes",
        )),
        _ => Err(ProtocolError::new("unknown command tag")),
    }
}

/// A player projection packed within [`MAX_PLAYER_PROJECTION_BYTES`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedSnapshot {
    pub payload: Vec<u8>,
    /// Leading entities of the projection that fit; the rest were omitted.
    pub packed_entities: usize,
}

/// Budget-driven packing: writes the header, then entities in the projection's
/// priority order until the next record would exceed
/// [`MAX_PLAYER_PROJECTION_BYTES`]. The viewer leads every projection and must
/// always fit; later higher-priority sections (and the viewer's current target)
/// will be written before the remaining entities. Positions outside the `i16`
/// range fail closed rather than being clamped.
pub fn pack_snapshot(snapshot: &ZoneSnapshot) -> Result<PackedSnapshot, ProtocolError> {
    let viewer = (EntityKind::Player, snapshot.viewer_id);
    if snapshot
        .entities
        .first()
        .is_none_or(|entity| (entity.kind, entity.id) != viewer)
    {
        return Err(ProtocolError::new(
            "player projection must start with the viewer",
        ));
    }
    let mut payload = Vec::with_capacity(MAX_PLAYER_PROJECTION_BYTES);
    encode_common_header(
        &mut payload,
        PLAYER_SNAPSHOT_SCOPE,
        snapshot.schema_version,
        snapshot.zone_id,
        snapshot.tick,
    )?;
    payload.extend_from_slice(&snapshot.content_revision.to_be_bytes());
    payload.extend_from_slice(&snapshot.acknowledged_sequence.to_be_bytes());
    payload.extend_from_slice(&snapshot.viewer_id.to_be_bytes());
    let count_offset = payload.len();
    payload.extend_from_slice(&0_u16.to_be_bytes());

    let mut packed_entities = 0_u16;
    for entity in &snapshot.entities {
        if payload.len() + ENTITY_RECORD_BYTES > MAX_PLAYER_PROJECTION_BYTES {
            break;
        }
        encode_entity(&mut payload, entity)?;
        packed_entities += 1;
    }
    if packed_entities == 0 {
        return Err(ProtocolError::new(
            "player projection budget cannot hold the viewer",
        ));
    }
    payload[count_offset..count_offset + 2].copy_from_slice(&packed_entities.to_be_bytes());
    Ok(PackedSnapshot {
        payload,
        packed_entities: usize::from(packed_entities),
    })
}

/// Encodes a complete player projection. Fails closed, never truncating
/// silently, when any entity would not fit [`MAX_PLAYER_PROJECTION_BYTES`].
pub fn encode_snapshot(snapshot: &ZoneSnapshot) -> Result<Vec<u8>, ProtocolError> {
    let packed = pack_snapshot(snapshot)?;
    if packed.packed_entities < snapshot.entities.len() {
        return Err(ProtocolError::new(
            "player projection exceeds the byte budget",
        ));
    }
    Ok(packed.payload)
}

fn encode_entity(payload: &mut Vec<u8>, entity: &EntitySnapshot) -> Result<(), ProtocolError> {
    payload.push(entity_kind_code(entity.kind));
    payload.extend_from_slice(&entity.id.to_be_bytes());
    for component in entity.position {
        let component = i16::try_from(component)
            .map_err(|_| ProtocolError::new("entity position is outside the compact wire range"))?;
        payload.extend_from_slice(&component.to_be_bytes());
    }
    for component in entity.velocity {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.extend_from_slice(&entity.facing.to_be_bytes());
    Ok(())
}

pub fn encode_canonical_snapshot(
    snapshot: &CanonicalZoneSnapshot,
) -> Result<Vec<u8>, ProtocolError> {
    let count = encode_count(
        snapshot.players.len(),
        MAX_PLAYERS_PER_ZONE,
        "snapshot exceeds configured zone player capacity",
    )?;
    let capacity = snapshot
        .players
        .len()
        .checked_mul(CANONICAL_PLAYER_RECORD_BYTES)
        .and_then(|bytes| bytes.checked_add(COMMON_HEADER_BYTES + 2))
        .ok_or_else(|| ProtocolError::new("snapshot size overflow"))?;
    let mut payload = Vec::with_capacity(capacity);
    encode_common_header(
        &mut payload,
        CANONICAL_SNAPSHOT_SCOPE,
        snapshot.schema_version,
        snapshot.zone_id,
        snapshot.tick,
    )?;
    payload.extend_from_slice(&count.to_be_bytes());
    encode_definition(&mut payload, &snapshot.definition);

    for player in &snapshot.players {
        payload.extend_from_slice(&player.player_id.to_be_bytes());
        for component in player.position.into_iter().chain(player.velocity) {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        payload.extend_from_slice(&player.facing.to_be_bytes());
        payload.extend_from_slice(&player.forward.to_be_bytes());
        payload.extend_from_slice(&player.strafe.to_be_bytes());
        payload.push(u8::from(player.jump_pending));
        payload.extend_from_slice(&player.last_sequence.to_be_bytes());
        payload.extend_from_slice(&player.spawn_slot.to_be_bytes());
    }
    Ok(payload)
}

pub fn decode_snapshot(payload: &[u8]) -> Result<ZoneSnapshot, ProtocolError> {
    if payload.len() > MAX_PLAYER_PROJECTION_BYTES {
        return Err(ProtocolError::new(
            "snapshot exceeds the player projection byte budget",
        ));
    }
    let mut offset = 0;
    let (schema_version, zone_id, tick) =
        decode_common_header(payload, &mut offset, PLAYER_SNAPSHOT_SCOPE)?;
    let content_revision = u64::from_be_bytes(take(payload, &mut offset)?);
    let acknowledged_sequence = u32::from_be_bytes(take(payload, &mut offset)?);
    let viewer_id = u32::from_be_bytes(take(payload, &mut offset)?);
    let count = decode_count(
        payload,
        &mut offset,
        MAX_WIRE_ENTITIES,
        "snapshot exceeds configured visible entity capacity",
    )?;

    let mut entities = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = decode_entity_kind(read_u8(payload, &mut offset)?)?;
        let id = u32::from_be_bytes(take(payload, &mut offset)?);
        let position = [
            i16::from_be_bytes(take(payload, &mut offset)?),
            i16::from_be_bytes(take(payload, &mut offset)?),
            i16::from_be_bytes(take(payload, &mut offset)?),
        ]
        .map(i32::from);
        let velocity = [
            i8::from_be_bytes(take(payload, &mut offset)?),
            i8::from_be_bytes(take(payload, &mut offset)?),
            i8::from_be_bytes(take(payload, &mut offset)?),
        ];
        let facing = u16::from_be_bytes(take(payload, &mut offset)?);
        entities.push(EntitySnapshot {
            kind,
            id,
            position,
            velocity,
            facing,
        });
    }

    ensure_fully_consumed(payload, offset)?;
    if entities
        .first()
        .is_none_or(|entity| (entity.kind, entity.id) != (EntityKind::Player, viewer_id))
    {
        return Err(ProtocolError::new(
            "player projection must start with the viewer",
        ));
    }

    Ok(ZoneSnapshot {
        content_revision,
        acknowledged_sequence,
        viewer_id,
        schema_version,
        zone_id,
        tick,
        entities,
    })
}

pub fn decode_canonical_snapshot(payload: &[u8]) -> Result<CanonicalZoneSnapshot, ProtocolError> {
    let mut offset = 0;
    let (schema_version, zone_id, tick) =
        decode_common_header(payload, &mut offset, CANONICAL_SNAPSHOT_SCOPE)?;
    let player_count = decode_count(
        payload,
        &mut offset,
        MAX_PLAYERS_PER_ZONE,
        "snapshot exceeds configured zone player capacity",
    )?;
    let definition = decode_definition(payload, &mut offset)?;

    let mut players = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        let player_id = u32::from_be_bytes(take(payload, &mut offset)?);
        let position = read_vector(payload, &mut offset)?;
        let velocity = read_vector(payload, &mut offset)?;
        let facing = u16::from_be_bytes(take(payload, &mut offset)?);
        let forward = i8::from_be_bytes(take(payload, &mut offset)?);
        let strafe = i8::from_be_bytes(take(payload, &mut offset)?);
        let jump_pending = match read_u8(payload, &mut offset)? {
            0 => false,
            1 => true,
            _ => return Err(ProtocolError::new("jump flag must be 0 or 1")),
        };
        let last_sequence = u32::from_be_bytes(take(payload, &mut offset)?);
        let spawn_slot = u16::from_be_bytes(take(payload, &mut offset)?);
        players.push(CanonicalPlayerSnapshot {
            player_id,
            position,
            velocity,
            facing,
            forward,
            strafe,
            jump_pending,
            last_sequence,
            spawn_slot,
        });
    }

    ensure_fully_consumed(payload, offset)?;

    Ok(CanonicalZoneSnapshot {
        definition,
        schema_version,
        zone_id,
        tick,
        players,
    })
}

const fn entity_kind_code(kind: EntityKind) -> u8 {
    match kind {
        EntityKind::Player => PLAYER_ENTITY_KIND,
    }
}

fn decode_entity_kind(code: u8) -> Result<EntityKind, ProtocolError> {
    match code {
        PLAYER_ENTITY_KIND => Ok(EntityKind::Player),
        _ => Err(ProtocolError::new("unknown entity kind")),
    }
}

fn encode_definition(payload: &mut Vec<u8>, definition: &ZoneDefinition) {
    payload.extend_from_slice(&definition.revision().to_be_bytes());
    for component in definition.gravity() {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    let spawn_grid = definition.spawn_grid();
    for component in spawn_grid.origin {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.extend_from_slice(&spawn_grid.columns.to_be_bytes());
    payload.extend_from_slice(&spawn_grid.spacing.to_be_bytes());
    let count = u16::try_from(definition.colliders().len())
        .expect("validated static collider capacity fits u16");
    payload.extend_from_slice(&count.to_be_bytes());
    for collider in definition.colliders() {
        payload.extend_from_slice(&collider.id.to_be_bytes());
        for component in collider.position.into_iter().chain(collider.half_extents) {
            payload.extend_from_slice(&component.to_be_bytes());
        }
    }
}

fn decode_definition(payload: &[u8], offset: &mut usize) -> Result<ZoneDefinition, ProtocolError> {
    let revision = u64::from_be_bytes(take(payload, offset)?);
    let gravity = read_vector(payload, offset)?;
    let spawn_grid = SpawnGrid {
        origin: [
            i32::from_be_bytes(take(payload, offset)?),
            i32::from_be_bytes(take(payload, offset)?),
        ],
        columns: u16::from_be_bytes(take(payload, offset)?),
        spacing: i32::from_be_bytes(take(payload, offset)?),
    };
    let count = usize::from(u16::from_be_bytes(take(payload, offset)?));
    if count > MAX_STATIC_COLLIDERS {
        return Err(ProtocolError::new("zone static collider capacity reached"));
    }
    let mut colliders = Vec::with_capacity(count);
    for _ in 0..count {
        colliders.push(StaticCollider {
            id: u32::from_be_bytes(take(payload, offset)?),
            position: read_vector(payload, offset)?,
            half_extents: read_vector(payload, offset)?,
        });
    }
    ZoneDefinition::with_spawn_grid(revision, gravity, spawn_grid, colliders)
        .map_err(|error| ProtocolError::new(error.to_string()))
}

fn read_vector(payload: &[u8], offset: &mut usize) -> Result<[i32; 3], ProtocolError> {
    Ok([
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
    ])
}

fn encode_common_header(
    payload: &mut Vec<u8>,
    scope: u8,
    schema_version: u16,
    zone_id: ZoneId,
    tick: u64,
) -> Result<(), ProtocolError> {
    if schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(ProtocolError::new(
            "unsupported core snapshot schema version",
        ));
    }
    payload.push(SNAPSHOT_WIRE_VERSION);
    payload.push(scope);
    payload.extend_from_slice(&schema_version.to_be_bytes());
    payload.extend_from_slice(&zone_id.get().to_be_bytes());
    payload.extend_from_slice(&tick.to_be_bytes());
    Ok(())
}

fn decode_common_header(
    payload: &[u8],
    offset: &mut usize,
    expected_scope: u8,
) -> Result<(u16, ZoneId, u64), ProtocolError> {
    if read_u8(payload, offset)? != SNAPSHOT_WIRE_VERSION {
        return Err(ProtocolError::new("unsupported snapshot wire version"));
    }
    if read_u8(payload, offset)? != expected_scope {
        return Err(ProtocolError::new("unexpected snapshot scope"));
    }
    let schema_version = u16::from_be_bytes(take(payload, offset)?);
    if schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(ProtocolError::new(
            "unsupported core snapshot schema version",
        ));
    }
    let zone_id = ZoneId::new(u32::from_be_bytes(take(payload, offset)?));
    let tick = u64::from_be_bytes(take(payload, offset)?);
    Ok((schema_version, zone_id, tick))
}

fn encode_count(count: usize, limit: usize, message: &str) -> Result<u16, ProtocolError> {
    if count > limit {
        return Err(ProtocolError::new(message));
    }
    u16::try_from(count).map_err(|_| ProtocolError::new(message))
}

/// Rejects excessive counts before any allocation sized by them.
fn decode_count(
    payload: &[u8],
    offset: &mut usize,
    limit: usize,
    message: &str,
) -> Result<usize, ProtocolError> {
    let count = usize::from(u16::from_be_bytes(take(payload, offset)?));
    if count > limit {
        return Err(ProtocolError::new(message));
    }
    Ok(count)
}

fn ensure_fully_consumed(payload: &[u8], offset: usize) -> Result<(), ProtocolError> {
    if offset != payload.len() {
        return Err(ProtocolError::new(
            "snapshot payload contains trailing bytes",
        ));
    }
    Ok(())
}

fn read_u8(payload: &[u8], offset: &mut usize) -> Result<u8, ProtocolError> {
    Ok(take::<1>(payload, offset)?[0])
}

fn take<const N: usize>(payload: &[u8], offset: &mut usize) -> Result<[u8; N], ProtocolError> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| ProtocolError::new("payload offset overflow"))?;
    let bytes = payload
        .get(*offset..end)
        .ok_or_else(|| ProtocolError::new("payload is truncated"))?;
    let array = bytes
        .try_into()
        .map_err(|_| ProtocolError::new("invalid payload width"))?;
    *offset = end;
    Ok(array)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: u32, position: [i32; 3], velocity: [i8; 3], facing: u16) -> EntitySnapshot {
        EntitySnapshot {
            kind: EntityKind::Player,
            id,
            position,
            velocity,
            facing,
        }
    }

    /// The shared Rust/browser golden projection.
    fn fixture_snapshot() -> ZoneSnapshot {
        ZoneSnapshot {
            content_revision: 42,
            acknowledged_sequence: 81,
            viewer_id: 7,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            entities: vec![
                player(7, [10, 20, -30], [1, -2, 3], 16_384),
                player(9, [-400, 90, 2_500], [-13, 16, i8::MIN], 49_152),
            ],
        }
    }

    fn fixture_bytes() -> Vec<u8> {
        let fixture = include_str!("../../../fixtures/protocol/player-snapshot-v4.hex").trim();
        (0..fixture.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&fixture[offset..offset + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn commands_have_exact_versioned_layouts() {
        let movement = ZoneCommand::Move {
            forward: -1,
            strafe: 1,
            facing: 0xabcd,
        };
        assert_eq!(encode_command(movement), [2, 1, 0xff, 1, 0xab, 0xcd]);
        assert_eq!(encode_command(ZoneCommand::Jump), [2, 2]);
        for command in [
            movement,
            ZoneCommand::Jump,
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: u16::MAX,
            },
        ] {
            assert_eq!(decode_command(&encode_command(command)).unwrap(), command);
        }
    }

    /// The shared Rust/browser command fixture: one command per line as
    /// `<hex> move <forward> <strafe> <facing>` or `<hex> jump`.
    fn command_fixture() -> String {
        let commands = [
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: 0,
            },
            ZoneCommand::Move {
                forward: 1,
                strafe: 1,
                facing: 16_384,
            },
            ZoneCommand::Move {
                forward: -1,
                strafe: -1,
                facing: 32_768,
            },
            ZoneCommand::Move {
                forward: 0,
                strafe: 1,
                facing: 0xabcd,
            },
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: u16::MAX,
            },
            ZoneCommand::Jump,
        ];
        let mut fixture = String::from(
            "# Command wire v2 golden fixture, verified by mmorpg-protocol and web tests.\n",
        );
        for command in commands {
            let hex = encode_command(command)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let fields = match command {
                ZoneCommand::Move {
                    forward,
                    strafe,
                    facing,
                } => format!("move {forward} {strafe} {facing}"),
                ZoneCommand::Jump => "jump".to_owned(),
            };
            fixture.push_str(&format!("{hex} {fields}\n"));
        }
        fixture
    }

    #[test]
    fn commands_match_the_shared_golden_fixture() {
        let checked_in = include_str!("../../../fixtures/protocol/commands-v2.hex");
        assert_eq!(checked_in, command_fixture());
        for line in checked_in.lines().filter(|line| !line.starts_with('#')) {
            let hex = line.split_whitespace().next().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(encode_command(decode_command(&bytes).unwrap()), bytes);
        }
    }

    #[test]
    fn command_decoding_is_strict() {
        let movement = encode_command(ZoneCommand::Move {
            forward: 1,
            strafe: -1,
            facing: 7,
        });
        for length in 0..movement.len() {
            assert!(decode_command(&movement[..length]).is_err(), "{length}");
        }
        let mut trailing = movement.clone();
        trailing.push(0);
        assert!(decode_command(&trailing).is_err());
        assert!(decode_command(&[2, 2, 0]).is_err(), "jump has no body");
        for unknown in [0, 3, u8::MAX] {
            assert_eq!(
                decode_command(&[2, unknown]).unwrap_err().to_string(),
                "unknown command tag"
            );
        }
        for version in [1, 3] {
            let mut other = movement.clone();
            other[0] = version;
            assert_eq!(
                decode_command(&other).unwrap_err().to_string(),
                "unsupported command wire version"
            );
        }
        assert!(
            decode_command(&[1, 1, 0, 1]).is_err(),
            "legacy v1 movement is rejected"
        );
        for (forward, strafe) in [(2, 0), (0, -2), (-128, 0), (0, 127)] {
            let mut invalid = movement.clone();
            invalid[2] = i8::to_be_bytes(forward)[0];
            invalid[3] = i8::to_be_bytes(strafe)[0];
            assert_eq!(
                decode_command(&invalid).unwrap_err().to_string(),
                "movement components must be between -1 and 1"
            );
        }
    }

    #[test]
    fn player_snapshot_matches_the_shared_golden_fixture() {
        let snapshot = fixture_snapshot();
        let encoded = encode_snapshot(&snapshot).unwrap();
        assert_eq!(encoded.len(), 34 + 2 * 16);
        assert_eq!(encoded, fixture_bytes());
        assert_eq!(decode_snapshot(&encoded).unwrap(), snapshot);
    }

    #[test]
    fn player_snapshot_decoder_is_strict() {
        let encoded = fixture_bytes();
        for length in 0..encoded.len() {
            assert!(decode_snapshot(&encoded[..length]).is_err(), "{length}");
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_snapshot(&trailing).unwrap_err().to_string(),
            "snapshot payload contains trailing bytes"
        );
        for (offset, value, message) in [
            (0, 3, "unsupported snapshot wire version"),
            (1, 1, "unexpected snapshot scope"),
            (3, 3, "unsupported core snapshot schema version"),
            (34, 0, "unknown entity kind"),
            (34, 2, "unknown entity kind"),
            (50, 3, "unknown entity kind"),
            (31, 9, "player projection must start with the viewer"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
        let mut swapped = encoded.clone();
        swapped[34..].rotate_left(ENTITY_RECORD_BYTES);
        assert_eq!(
            decode_snapshot(&swapped).unwrap_err().to_string(),
            "player projection must start with the viewer"
        );
        let mut fewer = encoded.clone();
        fewer[32..34].copy_from_slice(&1_u16.to_be_bytes());
        assert!(decode_snapshot(&fewer).is_err(), "count must match records");
        let mut more = encoded;
        more[32..34].copy_from_slice(&3_u16.to_be_bytes());
        assert!(decode_snapshot(&more).is_err(), "count must match records");
    }

    #[test]
    fn snapshot_decoder_rejects_counts_and_sizes_above_the_budget_before_allocation() {
        let mut encoded = encode_snapshot(&ZoneSnapshot {
            entities: vec![fixture_snapshot().entities[0].clone()],
            ..fixture_snapshot()
        })
        .unwrap();
        let excessive_count = u16::try_from(MAX_WIRE_ENTITIES + 1).unwrap();
        encoded[32..34].copy_from_slice(&excessive_count.to_be_bytes());
        assert_eq!(
            decode_snapshot(&encoded).unwrap_err().to_string(),
            "snapshot exceeds configured visible entity capacity"
        );
        let oversized = vec![0; MAX_PLAYER_PROJECTION_BYTES + 1];
        assert_eq!(
            decode_snapshot(&oversized).unwrap_err().to_string(),
            "snapshot exceeds the player projection byte budget"
        );
    }

    #[test]
    fn largest_projection_with_extreme_values_fits_the_datagram_budget() {
        assert_eq!(MAX_PLAYER_PROJECTION_BYTES, 1_161 - 20 - 64);
        let viewer_id = u32::MAX;
        let entities: Vec<_> = (0..MAX_VISIBLE_ENTITIES)
            .map(|index| {
                let offset = u32::try_from(index).unwrap();
                let (low, high) = if index % 2 == 0 {
                    (i16::MIN, i16::MAX)
                } else {
                    (i16::MAX, i16::MIN)
                };
                player(
                    viewer_id - offset,
                    [low, high, low].map(i32::from),
                    [i8::MIN, i8::MAX, i8::MIN],
                    u16::MAX,
                )
            })
            .collect();
        let snapshot = ZoneSnapshot {
            content_revision: u64::MAX,
            acknowledged_sequence: u32::MAX,
            viewer_id,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(u32::MAX),
            tick: u64::MAX,
            entities,
        };
        let encoded = encode_snapshot(&snapshot).unwrap();
        assert_eq!(
            encoded.len(),
            PLAYER_SNAPSHOT_HEADER_BYTES + MAX_VISIBLE_ENTITIES * ENTITY_RECORD_BYTES
        );
        assert!(encoded.len() <= MAX_PLAYER_PROJECTION_BYTES);
        assert!(
            encoded.len() + SESSION_SNAPSHOT_HEADER_BYTES + DATAGRAM_SAFETY_MARGIN_BYTES
                <= MEASURED_MIN_DATAGRAM_BYTES
        );
        assert_eq!(decode_snapshot(&encoded).unwrap(), snapshot);
    }

    #[test]
    fn packing_keeps_priority_order_within_the_budget_and_strict_encoding_fails_closed() {
        let entities: Vec<_> = (1..=100)
            .map(|id| player(id, [i32::try_from(id).unwrap(), 90, 0], [0; 3], 0))
            .collect();
        let snapshot = ZoneSnapshot {
            viewer_id: 1,
            entities,
            ..fixture_snapshot()
        };
        let packed = pack_snapshot(&snapshot).unwrap();
        assert_eq!(packed.packed_entities, MAX_WIRE_ENTITIES);
        assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
        let decoded = decode_snapshot(&packed.payload).unwrap();
        assert_eq!(decoded.entities, snapshot.entities[..MAX_WIRE_ENTITIES]);
        assert_eq!(
            encode_snapshot(&snapshot).unwrap_err().to_string(),
            "player projection exceeds the byte budget"
        );
    }

    #[test]
    fn snapshot_encoding_fails_closed_on_missing_viewer_or_wide_positions() {
        let mut no_viewer = fixture_snapshot();
        no_viewer.entities.clear();
        let mut viewer_second = fixture_snapshot();
        viewer_second.entities.reverse();
        for invalid in [no_viewer, viewer_second] {
            assert_eq!(
                encode_snapshot(&invalid).unwrap_err().to_string(),
                "player projection must start with the viewer"
            );
        }
        for component in [i32::from(i16::MAX) + 1, i32::from(i16::MIN) - 1] {
            let mut wide = fixture_snapshot();
            wide.entities[1].position[2] = component;
            assert_eq!(
                encode_snapshot(&wide).unwrap_err().to_string(),
                "entity position is outside the compact wire range"
            );
        }
    }

    #[test]
    fn canonical_snapshot_round_trip_preserves_continuation_state() {
        let snapshot = CanonicalZoneSnapshot {
            definition: ZoneDefinition::default(),
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            players: vec![
                CanonicalPlayerSnapshot {
                    player_id: 7,
                    position: [10, 20, -30],
                    velocity: [1, -2, 70_000],
                    facing: 0xfedc,
                    forward: -1,
                    strafe: 1,
                    jump_pending: true,
                    last_sequence: 81,
                    spawn_slot: 3,
                },
                CanonicalPlayerSnapshot {
                    player_id: 8,
                    position: [0; 3],
                    velocity: [0; 3],
                    facing: 0,
                    forward: 0,
                    strafe: 0,
                    jump_pending: false,
                    last_sequence: 0,
                    spawn_slot: 0,
                },
            ],
        };

        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(encoded.len(), 18 + 36 + 2 * 39);
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        assert_eq!(
            decode_snapshot(&encoded).unwrap_err().to_string(),
            "unexpected snapshot scope"
        );
        // Record 0 starts after the 18-byte header and the 36-byte empty definition.
        let jump_flag = 18 + 36 + 32;
        assert_eq!(encoded[jump_flag], 1);
        let mut invalid_flag = encoded.clone();
        invalid_flag[jump_flag] = 2;
        assert_eq!(
            decode_canonical_snapshot(&invalid_flag)
                .unwrap_err()
                .to_string(),
            "jump flag must be 0 or 1"
        );
        for (offset, value) in [(0, 3), (3, 3)] {
            let mut legacy = encoded.clone();
            legacy[offset] = value;
            assert!(
                decode_canonical_snapshot(&legacy).is_err(),
                "v3 recovery bytes are rejected"
            );
        }
    }

    #[test]
    fn canonical_wire_restores_physical_world_and_rejects_invalid_content() {
        let spawn_grid = SpawnGrid {
            origin: [-500, 300],
            columns: 8,
            spacing: 120,
        };
        let definition = ZoneDefinition::with_spawn_grid(
            7,
            [0, -1, 0],
            spawn_grid,
            vec![StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [1000, 50, 1000],
            }],
        )
        .unwrap();
        let mut zone =
            mmorpg_core::ZoneSimulation::with_definition(ZoneId::new(1), definition).unwrap();
        zone.add_player(1).unwrap();
        let mut state = zone.snapshot().unwrap();
        assert_eq!(state.players[0].position, [-500, 90, 300]);
        state.players[0].position[1] = 400;
        state.players[0].velocity[1] = -5;
        let encoded = encode_canonical_snapshot(&state).unwrap();
        let mut original = mmorpg_core::ZoneSimulation::from_snapshot(state).unwrap();
        let mut restored = mmorpg_core::ZoneSimulation::from_snapshot(
            decode_canonical_snapshot(&encoded).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.definition().spawn_grid(), spawn_grid);
        for _ in 0..40 {
            original.advance_tick().unwrap();
            restored.advance_tick().unwrap();
            assert_eq!(original.snapshot().unwrap(), restored.snapshot().unwrap());
        }
        restored.add_player(2).unwrap();
        assert_eq!(
            restored.snapshot().unwrap().players[1].position,
            [-380, 90, 300]
        );
        for length in 0..encoded.len() {
            assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_canonical_snapshot(&trailing).is_err());
        let mut excessive_players = encoded.clone();
        excessive_players[16..18].copy_from_slice(&u16::MAX.to_be_bytes());
        assert_eq!(
            decode_canonical_snapshot(&excessive_players)
                .unwrap_err()
                .to_string(),
            "snapshot exceeds configured zone player capacity"
        );
        // Header 18, revision 8, gravity 12, spawn grid 14, then the collider count.
        let mut excessive_colliders = encoded.clone();
        excessive_colliders[52..54].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(decode_canonical_snapshot(&excessive_colliders).is_err());
        let mut invalid_extent = encoded.clone();
        invalid_extent[70..74].copy_from_slice(&(-1_i32).to_be_bytes());
        assert!(decode_canonical_snapshot(&invalid_extent).is_err());
        let mut empty_spawn_grid = encoded.clone();
        empty_spawn_grid[46..48].copy_from_slice(&0_u16.to_be_bytes());
        assert_eq!(
            decode_canonical_snapshot(&empty_spawn_grid)
                .unwrap_err()
                .to_string(),
            "spawn grid needs columns and non-overlapping slots"
        );
        let mut legacy = encoded;
        legacy[0] = 1;
        assert!(decode_canonical_snapshot(&legacy).is_err());
    }
}
