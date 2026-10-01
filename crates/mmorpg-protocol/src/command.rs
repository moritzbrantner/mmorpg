//! Command wire version 4 adds ability use (tag 9), cast cancellation
//! (tag 10) and the class choice (tag 11).

use mmorpg_core::ZoneCommand;

use crate::ProtocolError;
use crate::wire::{decode_entity_ref, encode_entity_ref};

pub const COMMAND_WIRE_VERSION: u8 = 4;

const MOVE_TAG: u8 = 1;
const JUMP_TAG: u8 = 2;
const SELECT_TARGET_TAG: u8 = 3;
const START_ATTACK_TAG: u8 = 4;
const STOP_ATTACK_TAG: u8 = 5;
const RELEASE_SPIRIT_TAG: u8 = 6;
const MOVE_ITEM_TAG: u8 = 7;
const LOOT_TAG: u8 = 8;
const USE_ABILITY_TAG: u8 = 9;
const CANCEL_CAST_TAG: u8 = 10;
const CHOOSE_CLASS_TAG: u8 = 11;
const USE_ABILITY_COMMAND_BYTES: usize = 8;
const CHOOSE_CLASS_COMMAND_BYTES: usize = 4;
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
        ZoneCommand::Loot(claim) => {
            let mut payload = vec![COMMAND_WIRE_VERSION, LOOT_TAG];
            crate::loot::encode_claim(&mut payload, claim);
            payload
        }
        ZoneCommand::Jump => vec![COMMAND_WIRE_VERSION, JUMP_TAG],
        ZoneCommand::UseAbility { ability, target } => {
            let mut payload = Vec::with_capacity(USE_ABILITY_COMMAND_BYTES);
            payload.extend_from_slice(&[COMMAND_WIRE_VERSION, USE_ABILITY_TAG, ability]);
            encode_entity_ref(&mut payload, target);
            payload
        }
        ZoneCommand::CancelCast => vec![COMMAND_WIRE_VERSION, CANCEL_CAST_TAG],
        ZoneCommand::ChooseClass { class, sex } => {
            vec![COMMAND_WIRE_VERSION, CHOOSE_CLASS_TAG, class, sex]
        }
        ZoneCommand::SelectTarget(target) => {
            let mut payload = Vec::with_capacity(SELECT_TARGET_COMMAND_BYTES);
            payload.extend_from_slice(&[COMMAND_WIRE_VERSION, SELECT_TARGET_TAG]);
            encode_entity_ref(&mut payload, target);
            payload
        }
        ZoneCommand::StartAttack => vec![COMMAND_WIRE_VERSION, START_ATTACK_TAG],
        ZoneCommand::StopAttack => vec![COMMAND_WIRE_VERSION, STOP_ATTACK_TAG],
        ZoneCommand::ReleaseSpirit => vec![COMMAND_WIRE_VERSION, RELEASE_SPIRIT_TAG],
        ZoneCommand::MoveItem {
            source,
            destination,
            quantity,
        } => {
            let [high, low] = quantity.to_be_bytes();
            vec![
                COMMAND_WIRE_VERSION,
                MOVE_ITEM_TAG,
                source,
                destination,
                high,
                low,
            ]
        }
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
        LOOT_TAG => {
            if payload.len() != 14 {
                return Err(ProtocolError::new(
                    "loot command payload must be exactly 14 bytes",
                ));
            }
            let mut offset = BARE_COMMAND_BYTES;
            Ok(ZoneCommand::Loot(crate::loot::decode_claim(
                payload,
                &mut offset,
            )?))
        }
        MOVE_ITEM_TAG => {
            let [_, _, source, destination, high, low] = payload else {
                return Err(ProtocolError::new(
                    "move item command payload must be exactly 6 bytes",
                ));
            };
            Ok(ZoneCommand::MoveItem {
                source: *source,
                destination: *destination,
                quantity: u16::from_be_bytes([*high, *low]),
            })
        }
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
        USE_ABILITY_TAG => {
            if payload.len() != USE_ABILITY_COMMAND_BYTES {
                return Err(ProtocolError::new(
                    "use ability command payload must be exactly 8 bytes",
                ));
            }
            let mut offset = BARE_COMMAND_BYTES + 1;
            Ok(ZoneCommand::UseAbility {
                ability: payload[BARE_COMMAND_BYTES],
                target: decode_entity_ref(payload, &mut offset)?,
            })
        }
        CHOOSE_CLASS_TAG => {
            let [_, _, class, sex] = payload else {
                return Err(ProtocolError::new(
                    "choose class command payload must be exactly 4 bytes",
                ));
            };
            debug_assert_eq!(payload.len(), CHOOSE_CLASS_COMMAND_BYTES);
            Ok(ZoneCommand::ChooseClass {
                class: *class,
                sex: *sex,
            })
        }
        CANCEL_CAST_TAG => bare(ZoneCommand::CancelCast),
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
        assert_eq!(encode_command(movement), [4, 1, 0xff, 1, 0xab, 0xcd]);
        assert_eq!(encode_command(ZoneCommand::Jump), [4, 2]);
        assert_eq!(
            encode_command(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
                CreatureId::new(0x0102_0304)
            )))),
            [4, 3, 2, 1, 2, 3, 4]
        );
        assert_eq!(
            encode_command(ZoneCommand::SelectTarget(None)),
            [4, 3, 0, 0, 0, 0, 0]
        );
        assert_eq!(encode_command(ZoneCommand::StartAttack), [4, 4]);
        assert_eq!(encode_command(ZoneCommand::StopAttack), [4, 5]);
        assert_eq!(encode_command(ZoneCommand::ReleaseSpirit), [4, 6]);
        assert_eq!(
            encode_command(ZoneCommand::UseAbility {
                ability: 9,
                target: Some(EntityRef::Creature(CreatureId::new(0x0102_0304))),
            }),
            [4, 9, 9, 2, 1, 2, 3, 4]
        );
        assert_eq!(
            encode_command(ZoneCommand::UseAbility {
                ability: 3,
                target: None,
            }),
            [4, 9, 3, 0, 0, 0, 0, 0]
        );
        assert_eq!(encode_command(ZoneCommand::CancelCast), [4, 10]);
        assert_eq!(
            encode_command(ZoneCommand::ChooseClass { class: 2, sex: 1 }),
            [4, 11, 2, 1]
        );
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
            ZoneCommand::CancelCast,
            ZoneCommand::UseAbility {
                ability: u8::MAX,
                target: Some(EntityRef::Npc(NpcId::new(u32::MAX))),
            },
            // Out-of-range class values are gameplay refusals, not wire errors.
            ZoneCommand::ChooseClass {
                class: u8::MAX,
                sex: u8::MAX,
            },
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
            ZoneCommand::MoveItem {
                source: 0,
                destination: 15,
                quantity: 2,
            },
            ZoneCommand::MoveItem {
                source: 15,
                destination: 0,
                quantity: u16::MAX,
            },
            ZoneCommand::MoveItem {
                source: u8::MAX,
                destination: 0,
                quantity: 0,
            },
            ZoneCommand::Loot(mmorpg_core::LootClaim {
                creature: CreatureId::new(1),
                died_at: 99,
            }),
            ZoneCommand::Loot(mmorpg_core::LootClaim {
                creature: CreatureId::new(u32::MAX),
                died_at: u64::MAX,
            }),
            ZoneCommand::UseAbility {
                ability: 1,
                target: None,
            },
            ZoneCommand::UseAbility {
                ability: 9,
                target: Some(EntityRef::Creature(CreatureId::new(108))),
            },
            ZoneCommand::UseAbility {
                ability: u8::MAX,
                target: Some(EntityRef::Player(4_000_000_000)),
            },
            ZoneCommand::CancelCast,
            ZoneCommand::ChooseClass { class: 0, sex: 1 },
            ZoneCommand::ChooseClass { class: 2, sex: 0 },
            ZoneCommand::ChooseClass {
                class: u8::MAX,
                sex: 7,
            },
        ];
        let mut fixture = String::from(
            "# Command wire v4 golden fixture, verified by mmorpg-protocol and web tests.\n",
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
                ZoneCommand::Loot(claim) => {
                    format!("loot {} {}", claim.creature.get(), claim.died_at)
                }
                ZoneCommand::SelectTarget(target) => {
                    let (kind, id) = entity_fields(target);
                    format!("select_target {kind} {id}")
                }
                ZoneCommand::UseAbility { ability, target } => {
                    let (kind, id) = entity_fields(target);
                    format!("use_ability {ability} {kind} {id}")
                }
                ZoneCommand::CancelCast => "cancel_cast".to_owned(),
                ZoneCommand::ChooseClass { class, sex } => format!("choose_class {class} {sex}"),
                ZoneCommand::StartAttack => "start_attack".to_owned(),
                ZoneCommand::StopAttack => "stop_attack".to_owned(),
                ZoneCommand::ReleaseSpirit => "release_spirit".to_owned(),
                ZoneCommand::MoveItem {
                    source,
                    destination,
                    quantity,
                } => format!("move_item {source} {destination} {quantity}"),
            };
            fixture.push_str(&format!("{hex} {fields}\n"));
        }
        fixture
    }

    fn entity_fields(target: Option<EntityRef>) -> (u8, u32) {
        target.map_or((0, 0), |target| {
            let kind = match target.kind() {
                mmorpg_core::EntityKind::Player => 1,
                mmorpg_core::EntityKind::Creature => 2,
                mmorpg_core::EntityKind::Npc => 3,
            };
            (kind, target.id())
        })
    }

    #[test]
    fn commands_match_the_shared_golden_fixture() {
        let checked_in = include_str!("../../../fixtures/protocol/commands-v4.hex");
        if std::env::var_os("MMORPG_PRINT_FIXTURE").is_some() {
            print!("{}", command_fixture());
        }
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
    fn actual_legacy_v2_and_v3_command_fixtures_are_rejected() {
        for line in include_str!("../../../fixtures/protocol/commands-v2.hex")
            .lines()
            .chain(include_str!("../../../fixtures/protocol/commands-v3.hex").lines())
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let hex = line.split_whitespace().next().unwrap();
            let old = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                .collect::<Vec<_>>();
            assert!(decode_command(&old).is_err());
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
        let loot = encode_command(ZoneCommand::Loot(mmorpg_core::LootClaim {
            creature: CreatureId::new(1),
            died_at: 99,
        }));
        let ability = encode_command(ZoneCommand::UseAbility {
            ability: 5,
            target: Some(EntityRef::Creature(CreatureId::new(7))),
        });
        let class = encode_command(ZoneCommand::ChooseClass { class: 1, sex: 0 });
        for encoded in [&movement, &select, &loot, &ability, &class] {
            for length in 0..encoded.len() {
                assert!(decode_command(&encoded[..length]).is_err(), "{length}");
            }
            let mut trailing = encoded.clone();
            trailing.push(0);
            assert!(decode_command(&trailing).is_err());
        }
        for tag in [2, 4, 5, 6, 10] {
            assert_eq!(
                decode_command(&[4, tag, 0]).unwrap_err().to_string(),
                "command payload must be exactly 2 bytes",
                "tag {tag} has no body"
            );
        }
        for unknown in [0, 12, u8::MAX] {
            assert_eq!(
                decode_command(&[4, unknown]).unwrap_err().to_string(),
                "unknown command tag"
            );
        }
        for version in [1, 2, 3, 5] {
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
            decode_command(&[4, 3, 4, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "unknown entity kind"
        );
        assert_eq!(
            decode_command(&[4, 9, 1, 4, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "unknown entity kind"
        );
        assert_eq!(
            decode_command(&[4, 3, 0, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "an absent entity must have ID 0"
        );
    }
}
