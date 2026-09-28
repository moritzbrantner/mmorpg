//! Command wire version 2. Tags 3–6 were added for targeting and combat;
//! earlier tags are unchanged, so the version stays 2.

use mmorpg_core::ZoneCommand;

use crate::ProtocolError;
use crate::wire::{decode_entity_ref, encode_entity_ref};

pub const COMMAND_WIRE_VERSION: u8 = 2;

const MOVE_TAG: u8 = 1;
const JUMP_TAG: u8 = 2;
const SELECT_TARGET_TAG: u8 = 3;
const START_ATTACK_TAG: u8 = 4;
const STOP_ATTACK_TAG: u8 = 5;
const RELEASE_SPIRIT_TAG: u8 = 6;
const MOVE_COMMAND_BYTES: usize = 6;
const SELECT_TARGET_COMMAND_BYTES: usize = 7;
const BARE_COMMAND_BYTES: usize = 2;

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
        ZoneCommand::SelectTarget(target) => {
            let mut payload = Vec::with_capacity(SELECT_TARGET_COMMAND_BYTES);
            payload.extend_from_slice(&[COMMAND_WIRE_VERSION, SELECT_TARGET_TAG]);
            encode_entity_ref(&mut payload, target);
            payload
        }
        ZoneCommand::StartAttack => vec![COMMAND_WIRE_VERSION, START_ATTACK_TAG],
        ZoneCommand::StopAttack => vec![COMMAND_WIRE_VERSION, STOP_ATTACK_TAG],
        ZoneCommand::ReleaseSpirit => vec![COMMAND_WIRE_VERSION, RELEASE_SPIRIT_TAG],
    }
}

pub fn decode_command(payload: &[u8]) -> Result<ZoneCommand, ProtocolError> {
    let [version, tag, ..] = payload else {
        return Err(ProtocolError::new("command payload is truncated"));
    };
    if *version != COMMAND_WIRE_VERSION {
        return Err(ProtocolError::new("unsupported command wire version"));
    }
    let bare = |command| {
        if payload.len() == BARE_COMMAND_BYTES {
            Ok(command)
        } else {
            Err(ProtocolError::new(
                "command payload must be exactly 2 bytes",
            ))
        }
    };
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
        SELECT_TARGET_TAG => {
            if payload.len() != SELECT_TARGET_COMMAND_BYTES {
                return Err(ProtocolError::new(
                    "select target command payload must be exactly 7 bytes",
                ));
            }
            let mut offset = BARE_COMMAND_BYTES;
            Ok(ZoneCommand::SelectTarget(decode_entity_ref(
                payload,
                &mut offset,
            )?))
        }
        JUMP_TAG => bare(ZoneCommand::Jump),
        START_ATTACK_TAG => bare(ZoneCommand::StartAttack),
        STOP_ATTACK_TAG => bare(ZoneCommand::StopAttack),
        RELEASE_SPIRIT_TAG => bare(ZoneCommand::ReleaseSpirit),
        _ => Err(ProtocolError::new("unknown command tag")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureId, EntityRef, NpcId};

    #[test]
    fn commands_have_exact_versioned_layouts() {
        let movement = ZoneCommand::Move {
            forward: -1,
            strafe: 1,
            facing: 0xabcd,
        };
        assert_eq!(encode_command(movement), [2, 1, 0xff, 1, 0xab, 0xcd]);
        assert_eq!(encode_command(ZoneCommand::Jump), [2, 2]);
        assert_eq!(
            encode_command(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
                CreatureId::new(0x0102_0304)
            )))),
            [2, 3, 2, 1, 2, 3, 4]
        );
        assert_eq!(
            encode_command(ZoneCommand::SelectTarget(None)),
            [2, 3, 0, 0, 0, 0, 0]
        );
        assert_eq!(encode_command(ZoneCommand::StartAttack), [2, 4]);
        assert_eq!(encode_command(ZoneCommand::StopAttack), [2, 5]);
        assert_eq!(encode_command(ZoneCommand::ReleaseSpirit), [2, 6]);
        for command in [
            movement,
            ZoneCommand::Jump,
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: u16::MAX,
            },
            ZoneCommand::SelectTarget(Some(EntityRef::Player(u32::MAX))),
            ZoneCommand::SelectTarget(Some(EntityRef::Npc(NpcId::new(9)))),
            ZoneCommand::SelectTarget(None),
            ZoneCommand::StartAttack,
            ZoneCommand::StopAttack,
            ZoneCommand::ReleaseSpirit,
        ] {
            assert_eq!(decode_command(&encode_command(command)).unwrap(), command);
        }
    }

    /// The shared Rust/browser command fixture: one command per line as
    /// `<hex> <name> [fields]`.
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
            ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(108)))),
            ZoneCommand::SelectTarget(Some(EntityRef::Player(4_000_000_000))),
            ZoneCommand::SelectTarget(Some(EntityRef::Npc(NpcId::new(5)))),
            ZoneCommand::SelectTarget(None),
            ZoneCommand::StartAttack,
            ZoneCommand::StopAttack,
            ZoneCommand::ReleaseSpirit,
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
                ZoneCommand::SelectTarget(None) => "select_target 0 0".to_owned(),
                ZoneCommand::SelectTarget(Some(target)) => {
                    let kind = match target.kind() {
                        mmorpg_core::EntityKind::Player => 1,
                        mmorpg_core::EntityKind::Creature => 2,
                        mmorpg_core::EntityKind::Npc => 3,
                    };
                    format!("select_target {kind} {}", target.id())
                }
                ZoneCommand::StartAttack => "start_attack".to_owned(),
                ZoneCommand::StopAttack => "stop_attack".to_owned(),
                ZoneCommand::ReleaseSpirit => "release_spirit".to_owned(),
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
        let select = encode_command(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
            CreatureId::new(7),
        ))));
        for encoded in [&movement, &select] {
            for length in 0..encoded.len() {
                assert!(decode_command(&encoded[..length]).is_err(), "{length}");
            }
            let mut trailing = encoded.clone();
            trailing.push(0);
            assert!(decode_command(&trailing).is_err());
        }
        for tag in [2, 4, 5, 6] {
            assert_eq!(
                decode_command(&[2, tag, 0]).unwrap_err().to_string(),
                "command payload must be exactly 2 bytes",
                "tag {tag} has no body"
            );
        }
        for unknown in [0, 7, u8::MAX] {
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
        assert_eq!(
            decode_command(&[2, 3, 4, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "unknown entity kind"
        );
        assert_eq!(
            decode_command(&[2, 3, 0, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "an absent entity must have ID 0"
        );
    }
}
