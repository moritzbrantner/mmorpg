#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use mmorpg_core::{PlayerSnapshot, SNAPSHOT_SCHEMA_VERSION, ZoneCommand, ZoneId, ZoneSnapshot};

pub const COMMAND_WIRE_VERSION: u8 = 1;
pub const SNAPSHOT_WIRE_VERSION: u8 = 1;

const SET_MOVEMENT_TAG: u8 = 1;
const SNAPSHOT_HEADER_BYTES: usize = 17;
const PLAYER_SNAPSHOT_BYTES: usize = 16;

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
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(ProtocolError::new(
            "unsupported core snapshot schema version",
        ));
    }
    let player_count = u16::try_from(snapshot.players.len())
        .map_err(|_| ProtocolError::new("snapshot contains too many players"))?;
    let player_bytes = snapshot
        .players
        .len()
        .checked_mul(PLAYER_SNAPSHOT_BYTES)
        .ok_or_else(|| ProtocolError::new("snapshot size overflow"))?;
    let capacity = SNAPSHOT_HEADER_BYTES
        .checked_add(player_bytes)
        .ok_or_else(|| ProtocolError::new("snapshot size overflow"))?;

    let mut payload = Vec::with_capacity(capacity);
    payload.push(SNAPSHOT_WIRE_VERSION);
    payload.extend_from_slice(&snapshot.schema_version.to_be_bytes());
    payload.extend_from_slice(&snapshot.zone_id.get().to_be_bytes());
    payload.extend_from_slice(&snapshot.tick.to_be_bytes());
    payload.extend_from_slice(&player_count.to_be_bytes());

    for player in &snapshot.players {
        payload.extend_from_slice(&player.player_id.to_be_bytes());
        for component in player.position {
            payload.extend_from_slice(&component.to_be_bytes());
        }
    }
    Ok(payload)
}

pub fn decode_snapshot(payload: &[u8]) -> Result<ZoneSnapshot, ProtocolError> {
    let mut offset = 0;
    let wire_version = read_u8(payload, &mut offset)?;
    if wire_version != SNAPSHOT_WIRE_VERSION {
        return Err(ProtocolError::new("unsupported snapshot wire version"));
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

    if offset != payload.len() {
        return Err(ProtocolError::new(
            "snapshot payload contains trailing bytes",
        ));
    }

    Ok(ZoneSnapshot {
        schema_version,
        zone_id,
        tick,
        players,
    })
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
    fn snapshot_round_trip_preserves_authoritative_identity() {
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
