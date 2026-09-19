#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, MAX_PLAYERS_PER_ZONE, PlayerSnapshot,
    SNAPSHOT_SCHEMA_VERSION, ZoneCommand, ZoneId, ZoneSnapshot,
};

pub const COMMAND_WIRE_VERSION: u8 = 1;
pub const SNAPSHOT_WIRE_VERSION: u8 = 1;

const SET_MOVEMENT_TAG: u8 = 1;
const CANONICAL_SNAPSHOT_SCOPE: u8 = 1;
const PLAYER_SNAPSHOT_SCOPE: u8 = 2;
const SNAPSHOT_HEADER_BYTES: usize = 18;
const PLAYER_SNAPSHOT_BYTES: usize = 16;
const CANONICAL_PLAYER_SNAPSHOT_BYTES: usize = 24;

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
        ZoneCommand::SetMovement { x, z } => vec![
            COMMAND_WIRE_VERSION,
            SET_MOVEMENT_TAG,
            x.to_be_bytes()[0],
            z.to_be_bytes()[0],
        ],
    }
}

pub fn decode_command(payload: &[u8]) -> Result<ZoneCommand, ProtocolError> {
    if payload.len() != 4 {
        return Err(ProtocolError::new(
            "command payload must be exactly 4 bytes",
        ));
    }
    if payload[0] != COMMAND_WIRE_VERSION {
        return Err(ProtocolError::new("unsupported command wire version"));
    }
    if payload[1] != SET_MOVEMENT_TAG {
        return Err(ProtocolError::new("unknown command tag"));
    }

    let x = i8::from_be_bytes([payload[2]]);
    let z = i8::from_be_bytes([payload[3]]);
    if !(-1..=1).contains(&x) || !(-1..=1).contains(&z) {
        return Err(ProtocolError::new(
            "movement components must be between -1 and 1",
        ));
    }

    Ok(ZoneCommand::SetMovement { x, z })
}

pub fn encode_snapshot(snapshot: &ZoneSnapshot) -> Result<Vec<u8>, ProtocolError> {
    let mut payload = encode_snapshot_header(
        PLAYER_SNAPSHOT_SCOPE,
        snapshot.schema_version,
        snapshot.zone_id,
        snapshot.tick,
        snapshot.players.len(),
        PLAYER_SNAPSHOT_BYTES,
    )?;

    for player in &snapshot.players {
        payload.extend_from_slice(&player.player_id.to_be_bytes());
        for component in player.position {
            payload.extend_from_slice(&component.to_be_bytes());
        }
    }
    Ok(payload)
}

pub fn encode_canonical_snapshot(
    snapshot: &CanonicalZoneSnapshot,
) -> Result<Vec<u8>, ProtocolError> {
    let mut payload = encode_snapshot_header(
        CANONICAL_SNAPSHOT_SCOPE,
        snapshot.schema_version,
        snapshot.zone_id,
        snapshot.tick,
        snapshot.players.len(),
        CANONICAL_PLAYER_SNAPSHOT_BYTES,
    )?;

    for player in &snapshot.players {
        payload.extend_from_slice(&player.player_id.to_be_bytes());
        for component in player.position {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        payload.extend_from_slice(&player.movement_x.to_be_bytes());
        payload.extend_from_slice(&player.movement_z.to_be_bytes());
        payload.extend_from_slice(&player.last_sequence.to_be_bytes());
        payload.extend_from_slice(&player.spawn_slot.to_be_bytes());
    }
    Ok(payload)
}

pub fn decode_snapshot(payload: &[u8]) -> Result<ZoneSnapshot, ProtocolError> {
    let (schema_version, zone_id, tick, player_count, mut offset) =
        decode_snapshot_header(payload, PLAYER_SNAPSHOT_SCOPE)?;

    let mut players = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        let player_id = u32::from_be_bytes(take(payload, &mut offset)?);
        let x = i32::from_be_bytes(take(payload, &mut offset)?);
        let y = i32::from_be_bytes(take(payload, &mut offset)?);
        let z = i32::from_be_bytes(take(payload, &mut offset)?);
        players.push(PlayerSnapshot {
            player_id,
            position: [x, y, z],
        });
    }

    ensure_fully_consumed(payload, offset)?;

    Ok(ZoneSnapshot {
        schema_version,
        zone_id,
        tick,
        players,
    })
}

pub fn decode_canonical_snapshot(
    payload: &[u8],
) -> Result<CanonicalZoneSnapshot, ProtocolError> {
    let (schema_version, zone_id, tick, player_count, mut offset) =
        decode_snapshot_header(payload, CANONICAL_SNAPSHOT_SCOPE)?;

    let mut players = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        let player_id = u32::from_be_bytes(take(payload, &mut offset)?);
        let x = i32::from_be_bytes(take(payload, &mut offset)?);
        let y = i32::from_be_bytes(take(payload, &mut offset)?);
        let z = i32::from_be_bytes(take(payload, &mut offset)?);
        let movement_x = i8::from_be_bytes(take(payload, &mut offset)?);
        let movement_z = i8::from_be_bytes(take(payload, &mut offset)?);
        let last_sequence = u32::from_be_bytes(take(payload, &mut offset)?);
        let spawn_slot = u16::from_be_bytes(take(payload, &mut offset)?);
        players.push(CanonicalPlayerSnapshot {
            player_id,
            position: [x, y, z],
            movement_x,
            movement_z,
            last_sequence,
            spawn_slot,
        });
    }

    ensure_fully_consumed(payload, offset)?;

    Ok(CanonicalZoneSnapshot {
        schema_version,
        zone_id,
        tick,
        players,
    })
}

fn encode_snapshot_header(
    scope: u8,
    schema_version: u16,
    zone_id: ZoneId,
    tick: u64,
    player_count: usize,
    player_snapshot_bytes: usize,
) -> Result<Vec<u8>, ProtocolError> {
    if player_count > MAX_PLAYERS_PER_ZONE {
        return Err(ProtocolError::new(
            "snapshot exceeds configured zone player capacity",
        ));
    }
    if schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(ProtocolError::new(
            "unsupported core snapshot schema version",
        ));
    }
    let encoded_player_count = u16::try_from(player_count)
        .map_err(|_| ProtocolError::new("snapshot contains too many players"))?;
    let player_bytes = player_count
        .checked_mul(player_snapshot_bytes)
        .ok_or_else(|| ProtocolError::new("snapshot size overflow"))?;
    let capacity = SNAPSHOT_HEADER_BYTES
        .checked_add(player_bytes)
        .ok_or_else(|| ProtocolError::new("snapshot size overflow"))?;

    let mut payload = Vec::with_capacity(capacity);
    payload.push(SNAPSHOT_WIRE_VERSION);
    payload.push(scope);
    payload.extend_from_slice(&schema_version.to_be_bytes());
    payload.extend_from_slice(&zone_id.get().to_be_bytes());
    payload.extend_from_slice(&tick.to_be_bytes());
    payload.extend_from_slice(&encoded_player_count.to_be_bytes());
    Ok(payload)
}

fn decode_snapshot_header(
    payload: &[u8],
    expected_scope: u8,
) -> Result<(u16, ZoneId, u64, usize, usize), ProtocolError> {
    let mut offset = 0;
    let wire_version = read_u8(payload, &mut offset)?;
    if wire_version != SNAPSHOT_WIRE_VERSION {
        return Err(ProtocolError::new("unsupported snapshot wire version"));
    }
    if read_u8(payload, &mut offset)? != expected_scope {
        return Err(ProtocolError::new("unexpected snapshot scope"));
    }

    let schema_version = u16::from_be_bytes(take(payload, &mut offset)?);
    if schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(ProtocolError::new(
            "unsupported core snapshot schema version",
        ));
    }

    let zone_id = ZoneId::new(u32::from_be_bytes(take(payload, &mut offset)?));
    let tick = u64::from_be_bytes(take(payload, &mut offset)?);
    let player_count = usize::from(u16::from_be_bytes(take(payload, &mut offset)?));
    if player_count > MAX_PLAYERS_PER_ZONE {
        return Err(ProtocolError::new(
            "snapshot exceeds configured zone player capacity",
        ));
    }
    Ok((schema_version, zone_id, tick, player_count, offset))
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

    #[test]
    fn command_round_trip_is_strict_and_versioned() {
        let command = ZoneCommand::SetMovement { x: -1, z: 1 };
        assert_eq!(decode_command(&encode_command(command)).unwrap(), command);
        assert!(decode_command(&[2, SET_MOVEMENT_TAG, 0, 0]).is_err());
        assert!(decode_command(&[1, SET_MOVEMENT_TAG, 2, 0]).is_err());
    }

    #[test]
    fn player_snapshot_round_trip_preserves_projection() {
        let snapshot = ZoneSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            players: vec![PlayerSnapshot {
                player_id: 7,
                position: [10, 20, -30],
            }],
        };

        let encoded = encode_snapshot(&snapshot).unwrap();
        assert_eq!(decode_snapshot(&encoded).unwrap(), snapshot);
    }

    #[test]
    fn canonical_snapshot_round_trip_preserves_continuation_state() {
        let snapshot = CanonicalZoneSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            players: vec![CanonicalPlayerSnapshot {
                player_id: 7,
                position: [10, 20, -30],
                movement_x: -1,
                movement_z: 1,
                last_sequence: 81,
                spawn_slot: 3,
            }],
        };

        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        assert_eq!(
            decode_snapshot(&encoded).unwrap_err().to_string(),
            "unexpected snapshot scope"
        );
    }

    #[test]
    fn snapshot_decoder_rejects_counts_above_zone_capacity_before_allocation() {
        let snapshot = ZoneSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(1),
            tick: 1,
            players: Vec::new(),
        };
        let mut encoded = encode_snapshot(&snapshot).unwrap();
        let excessive_count =
            u16::try_from(MAX_PLAYERS_PER_ZONE + 1).expect("configured capacity fits u16");
        encoded[16..18].copy_from_slice(&excessive_count.to_be_bytes());

        let error = decode_snapshot(&encoded).unwrap_err();

        assert_eq!(
            error.to_string(),
            "snapshot exceeds configured zone player capacity"
        );
    }

    #[test]
    fn snapshot_decoder_rejects_truncation_and_trailing_bytes() {
        let snapshot = ZoneSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(1),
            tick: 1,
            players: Vec::new(),
        };
        let encoded = encode_snapshot(&snapshot).unwrap();

        assert!(decode_snapshot(&encoded[..encoded.len() - 1]).is_err());
        let mut trailing = encoded;
        trailing.push(0);
        assert!(decode_snapshot(&trailing).is_err());
    }
}
