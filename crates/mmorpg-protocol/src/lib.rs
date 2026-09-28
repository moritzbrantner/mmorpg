#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, EntityKind, EntitySnapshot,
    MAX_PLAYERS_PER_ZONE, MAX_STATIC_COLLIDERS, SNAPSHOT_SCHEMA_VERSION, StaticCollider,
    ZoneCommand, ZoneDefinition, ZoneId, ZoneSnapshot,
};

pub const COMMAND_WIRE_VERSION: u8 = 2;
pub const SNAPSHOT_WIRE_VERSION: u8 = 3;

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
const PLAYER_SNAPSHOT_HEADER_BYTES: usize = 34;
const ENTITY_RECORD_BYTES: usize = 25;
const CANONICAL_PLAYER_RECORD_BYTES: usize = 39;
/// Visible projections are bounded by the zone population until other unit
/// kinds exist and a projection cap replaces this bound.
const MAX_VISIBLE_ENTITIES: usize = MAX_PLAYERS_PER_ZONE;

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

pub fn encode_snapshot(snapshot: &ZoneSnapshot) -> Result<Vec<u8>, ProtocolError> {
    let count = encode_count(
        snapshot.entities.len(),
        MAX_VISIBLE_ENTITIES,
        "snapshot exceeds configured visible entity capacity",
    )?;
    let capacity = snapshot
        .entities
        .len()
        .checked_mul(ENTITY_RECORD_BYTES)
        .and_then(|bytes| bytes.checked_add(PLAYER_SNAPSHOT_HEADER_BYTES))
        .ok_or_else(|| ProtocolError::new("snapshot size overflow"))?;
    let mut payload = Vec::with_capacity(capacity);
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
    payload.extend_from_slice(&count.to_be_bytes());

    for entity in &snapshot.entities {
        payload.push(entity_kind_code(entity.kind));
        payload.extend_from_slice(&entity.id.to_be_bytes());
        for component in entity.position {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        for component in entity.velocity {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        payload.extend_from_slice(&entity.facing.to_be_bytes());
    }
    Ok(payload)
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
    let mut offset = 0;
    let (schema_version, zone_id, tick) =
        decode_common_header(payload, &mut offset, PLAYER_SNAPSHOT_SCOPE)?;
    let content_revision = u64::from_be_bytes(take(payload, &mut offset)?);
    let acknowledged_sequence = u32::from_be_bytes(take(payload, &mut offset)?);
    let viewer_id = u32::from_be_bytes(take(payload, &mut offset)?);
    let count = decode_count(
        payload,
        &mut offset,
        MAX_VISIBLE_ENTITIES,
        "snapshot exceeds configured visible entity capacity",
    )?;

    let mut entities = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = decode_entity_kind(read_u8(payload, &mut offset)?)?;
        let id = u32::from_be_bytes(take(payload, &mut offset)?);
        let position = read_vector(payload, &mut offset)?;
        let velocity = [
            i16::from_be_bytes(take(payload, &mut offset)?),
            i16::from_be_bytes(take(payload, &mut offset)?),
            i16::from_be_bytes(take(payload, &mut offset)?),
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
    ZoneDefinition::new(revision, gravity, colliders)
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

    fn player(id: u32, position: [i32; 3], velocity: [i16; 3], facing: u16) -> EntitySnapshot {
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
                player(9, [-400, 90, 2_500], [-13, 16, i16::MIN], 49_152),
            ],
        }
    }

    fn fixture_bytes() -> Vec<u8> {
        let fixture = include_str!("../../../fixtures/protocol/player-snapshot-v3.hex").trim();
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
        assert_eq!(encoded.len(), 34 + 2 * 25);
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
            (0, 2, "unsupported snapshot wire version"),
            (1, 1, "unexpected snapshot scope"),
            (3, 2, "unsupported core snapshot schema version"),
            (34, 0, "unknown entity kind"),
            (34, 2, "unknown entity kind"),
            (59, 3, "unknown entity kind"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
        let mut fewer = encoded.clone();
        fewer[32..34].copy_from_slice(&1_u16.to_be_bytes());
        assert!(decode_snapshot(&fewer).is_err(), "count must match records");
        let mut more = encoded;
        more[32..34].copy_from_slice(&3_u16.to_be_bytes());
        assert!(decode_snapshot(&more).is_err(), "count must match records");
    }

    #[test]
    fn snapshot_decoder_rejects_counts_above_capacity_before_allocation() {
        let mut encoded = encode_snapshot(&ZoneSnapshot {
            entities: Vec::new(),
            ..fixture_snapshot()
        })
        .unwrap();
        let excessive_count =
            u16::try_from(MAX_VISIBLE_ENTITIES + 1).expect("configured capacity fits u16");
        encoded[32..34].copy_from_slice(&excessive_count.to_be_bytes());

        let error = decode_snapshot(&encoded).unwrap_err();

        assert_eq!(
            error.to_string(),
            "snapshot exceeds configured visible entity capacity"
        );
        let oversized = ZoneSnapshot {
            entities: vec![player(1, [0; 3], [0; 3], 0); MAX_VISIBLE_ENTITIES + 1],
            ..fixture_snapshot()
        };
        assert!(encode_snapshot(&oversized).is_err());
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
        assert_eq!(encoded.len(), 18 + 22 + 2 * 39);
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        assert_eq!(
            decode_snapshot(&encoded).unwrap_err().to_string(),
            "unexpected snapshot scope"
        );
        // Record 0 starts after the 18-byte header and the 22-byte empty definition.
        let jump_flag = 18 + 22 + 32;
        assert_eq!(encoded[jump_flag], 1);
        let mut invalid_flag = encoded.clone();
        invalid_flag[jump_flag] = 2;
        assert_eq!(
            decode_canonical_snapshot(&invalid_flag)
                .unwrap_err()
                .to_string(),
            "jump flag must be 0 or 1"
        );
        for (offset, value) in [(0, 2), (3, 2)] {
            let mut legacy = encoded.clone();
            legacy[offset] = value;
            assert!(
                decode_canonical_snapshot(&legacy).is_err(),
                "v2 recovery bytes are rejected"
            );
        }
    }

    #[test]
    fn canonical_wire_restores_physical_world_and_rejects_invalid_content() {
        let definition = ZoneDefinition::new(
            7,
            [0, -1, 0],
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
        state.players[0].position[1] = 400;
        state.players[0].velocity[1] = -5;
        let encoded = encode_canonical_snapshot(&state).unwrap();
        let mut original = mmorpg_core::ZoneSimulation::from_snapshot(state).unwrap();
        let mut restored = mmorpg_core::ZoneSimulation::from_snapshot(
            decode_canonical_snapshot(&encoded).unwrap(),
        )
        .unwrap();
        for _ in 0..40 {
            original.advance_tick().unwrap();
            restored.advance_tick().unwrap();
            assert_eq!(original.snapshot().unwrap(), restored.snapshot().unwrap());
        }
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
        let mut excessive_colliders = encoded.clone();
        excessive_colliders[38..40].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(decode_canonical_snapshot(&excessive_colliders).is_err());
        let mut invalid_extent = encoded.clone();
        invalid_extent[56..60].copy_from_slice(&(-1_i32).to_be_bytes());
        assert!(decode_canonical_snapshot(&invalid_extent).is_err());
        let mut legacy = encoded;
        legacy[0] = 1;
        assert!(decode_canonical_snapshot(&legacy).is_err());
    }
}
