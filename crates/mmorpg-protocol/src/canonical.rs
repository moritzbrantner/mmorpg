//! Canonical scope: complete continuation state for trusted replay and
//! recovery. Content is referenced by revision and fingerprint; the decoder
//! checks structure, and `ZoneSimulation::from_snapshot` checks the state
//! against the content.

use mmorpg_core::{
    CanonicalCreatureSnapshot, CanonicalPlayerCombat, CanonicalPlayerSnapshot,
    CanonicalZoneSnapshot, CreatureAi, CreatureId, CreatureLife, EntityRef, MAX_CREATURE_SPAWNS,
    MAX_EVENTS_PER_PLAYER, MAX_PENDING_INTENTS, MAX_PLAYERS_PER_ZONE, MAX_THREAT_ENTRIES,
    PlayerIntent, ThreatEntry,
};

use crate::ProtocolError;
use crate::wire::{
    CANONICAL_SNAPSHOT_SCOPE, decode_common_header, decode_entity_ref, decode_event,
    decode_u8_count, decode_u16_count, encode_common_header, encode_entity_ref, encode_event,
    encode_u8_count, encode_u16_count, ensure_fully_consumed, read_bool, read_u8, read_vector,
    take,
};

const SELECT_TARGET_INTENT: u8 = 1;
const START_ATTACK_INTENT: u8 = 2;
const STOP_ATTACK_INTENT: u8 = 3;
const RELEASE_SPIRIT_INTENT: u8 = 4;
const ALIVE: u8 = 1;
const CORPSE: u8 = 2;
const DESPAWNED: u8 = 3;
const IDLE: u8 = 1;
const ENGAGED: u8 = 2;
const EVADING: u8 = 3;

pub fn encode_canonical_snapshot(
    snapshot: &CanonicalZoneSnapshot,
) -> Result<Vec<u8>, ProtocolError> {
    let player_count = encode_u16_count(
        snapshot.players.len(),
        MAX_PLAYERS_PER_ZONE,
        "snapshot exceeds configured zone player capacity",
    )?;
    let creature_count = encode_u16_count(
        snapshot.creatures.len(),
        MAX_CREATURE_SPAWNS,
        "snapshot exceeds the creature spawn capacity",
    )?;
    let mut payload = Vec::new();
    encode_common_header(
        &mut payload,
        CANONICAL_SNAPSHOT_SCOPE,
        snapshot.schema_version,
        snapshot.zone_id,
        snapshot.tick,
    )?;
    payload.extend_from_slice(&snapshot.content_revision.to_be_bytes());
    payload.extend_from_slice(&snapshot.content_fingerprint.to_be_bytes());
    payload.extend_from_slice(&snapshot.rng_state.to_be_bytes());
    payload.extend_from_slice(&player_count.to_be_bytes());
    for player in &snapshot.players {
        encode_player(&mut payload, player)?;
    }
    payload.extend_from_slice(&creature_count.to_be_bytes());
    for creature in &snapshot.creatures {
        encode_creature(&mut payload, creature)?;
    }
    Ok(payload)
}

fn encode_player(
    payload: &mut Vec<u8>,
    player: &CanonicalPlayerSnapshot,
) -> Result<(), ProtocolError> {
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
    let combat = &player.combat;
    payload.push(combat.level);
    payload.extend_from_slice(&combat.health.to_be_bytes());
    encode_entity_ref(payload, combat.target);
    payload.push(u8::from(combat.auto_attack));
    for timer in [
        combat.swing_timer,
        combat.combat_timer,
        combat.calm_ticks,
        combat.error_cooldown,
    ] {
        payload.extend_from_slice(&timer.to_be_bytes());
    }
    payload.push(encode_u8_count(
        combat.intents.len(),
        MAX_PENDING_INTENTS,
        "player has too many pending intents",
    )?);
    for intent in &combat.intents {
        let (code, target) = match *intent {
            PlayerIntent::SelectTarget(target) => (SELECT_TARGET_INTENT, target),
            PlayerIntent::StartAttack => (START_ATTACK_INTENT, None),
            PlayerIntent::StopAttack => (STOP_ATTACK_INTENT, None),
            PlayerIntent::ReleaseSpirit => (RELEASE_SPIRIT_INTENT, None),
        };
        payload.push(code);
        encode_entity_ref(payload, target);
    }
    payload.push(encode_u8_count(
        combat.events.len(),
        MAX_EVENTS_PER_PLAYER,
        "player has too many events",
    )?);
    for event in &combat.events {
        encode_event(payload, event);
    }
    Ok(())
}

fn encode_creature(
    payload: &mut Vec<u8>,
    creature: &CanonicalCreatureSnapshot,
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&creature.creature_id.get().to_be_bytes());
    payload.push(creature.level);
    payload.extend_from_slice(&creature.health.to_be_bytes());
    payload.extend_from_slice(&creature.facing.to_be_bytes());
    for component in creature.position.into_iter().chain(creature.velocity) {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    let (life, died_at) = match creature.life {
        CreatureLife::Alive => (ALIVE, 0),
        CreatureLife::Corpse { died_at } => (CORPSE, died_at),
        CreatureLife::Despawned { died_at } => (DESPAWNED, died_at),
    };
    payload.push(life);
    payload.extend_from_slice(&died_at.to_be_bytes());
    let (ai, timer, destination) = match creature.ai {
        CreatureAi::Idle { timer, destination } => (IDLE, timer, destination),
        CreatureAi::Engaged => (ENGAGED, 0, None),
        CreatureAi::Evading { ticks } => (EVADING, ticks, None),
    };
    payload.push(ai);
    payload.extend_from_slice(&timer.to_be_bytes());
    payload.push(u8::from(destination.is_some()));
    for component in destination.unwrap_or([0; 2]) {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.push(encode_u8_count(
        creature.threat.len(),
        MAX_THREAT_ENTRIES,
        "creature threat table exceeds its capacity",
    )?);
    for entry in &creature.threat {
        encode_entity_ref(payload, Some(entry.entity));
        payload.extend_from_slice(&entry.threat.to_be_bytes());
    }
    payload.extend_from_slice(&creature.swing_timer.to_be_bytes());
    payload.extend_from_slice(&creature.combat_timer.to_be_bytes());
    payload.push(u8::from(creature.tapped_by.is_some()));
    payload.extend_from_slice(&creature.tapped_by.unwrap_or(0).to_be_bytes());
    Ok(())
}

pub fn decode_canonical_snapshot(payload: &[u8]) -> Result<CanonicalZoneSnapshot, ProtocolError> {
    let mut offset = 0;
    let (schema_version, zone_id, tick) =
        decode_common_header(payload, &mut offset, CANONICAL_SNAPSHOT_SCOPE)?;
    let content_revision = u64::from_be_bytes(take(payload, &mut offset)?);
    let content_fingerprint = u64::from_be_bytes(take(payload, &mut offset)?);
    let rng_state = u64::from_be_bytes(take(payload, &mut offset)?);
    let player_count = decode_u16_count(
        payload,
        &mut offset,
        MAX_PLAYERS_PER_ZONE,
        "snapshot exceeds configured zone player capacity",
    )?;
    let mut players = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        players.push(decode_player(payload, &mut offset)?);
    }
    let creature_count = decode_u16_count(
        payload,
        &mut offset,
        MAX_CREATURE_SPAWNS,
        "snapshot exceeds the creature spawn capacity",
    )?;
    let mut creatures = Vec::with_capacity(creature_count);
    for _ in 0..creature_count {
        creatures.push(decode_creature(payload, &mut offset)?);
    }
    ensure_fully_consumed(payload, offset)?;
    Ok(CanonicalZoneSnapshot {
        schema_version,
        zone_id,
        tick,
        content_revision,
        content_fingerprint,
        rng_state,
        players,
        creatures,
    })
}

fn decode_player(
    payload: &[u8],
    offset: &mut usize,
) -> Result<CanonicalPlayerSnapshot, ProtocolError> {
    let player_id = u32::from_be_bytes(take(payload, offset)?);
    let position = read_vector(payload, offset)?;
    let velocity = read_vector(payload, offset)?;
    let facing = u16::from_be_bytes(take(payload, offset)?);
    let forward = i8::from_be_bytes(take(payload, offset)?);
    let strafe = i8::from_be_bytes(take(payload, offset)?);
    let jump_pending = match read_u8(payload, offset)? {
        0 => false,
        1 => true,
        _ => return Err(ProtocolError::new("jump flag must be 0 or 1")),
    };
    let last_sequence = u32::from_be_bytes(take(payload, offset)?);
    let spawn_slot = u16::from_be_bytes(take(payload, offset)?);
    let level = read_u8(payload, offset)?;
    let health = u32::from_be_bytes(take(payload, offset)?);
    let target = decode_entity_ref(payload, offset)?;
    let auto_attack = read_bool(payload, offset)?;
    let swing_timer = u16::from_be_bytes(take(payload, offset)?);
    let combat_timer = u16::from_be_bytes(take(payload, offset)?);
    let calm_ticks = u16::from_be_bytes(take(payload, offset)?);
    let error_cooldown = u16::from_be_bytes(take(payload, offset)?);
    let intent_count = decode_u8_count(
        payload,
        offset,
        MAX_PENDING_INTENTS,
        "player has too many pending intents",
    )?;
    let mut intents = Vec::with_capacity(intent_count);
    for _ in 0..intent_count {
        let code = read_u8(payload, offset)?;
        let target = decode_entity_ref(payload, offset)?;
        intents.push(match (code, target) {
            (SELECT_TARGET_INTENT, target) => PlayerIntent::SelectTarget(target),
            (START_ATTACK_INTENT, None) => PlayerIntent::StartAttack,
            (STOP_ATTACK_INTENT, None) => PlayerIntent::StopAttack,
            (RELEASE_SPIRIT_INTENT, None) => PlayerIntent::ReleaseSpirit,
            _ => return Err(ProtocolError::new("malformed pending intent")),
        });
    }
    let event_count = decode_u8_count(
        payload,
        offset,
        MAX_EVENTS_PER_PLAYER,
        "player has too many events",
    )?;
    let mut events = Vec::with_capacity(event_count);
    for _ in 0..event_count {
        events.push(decode_event(payload, offset)?);
    }
    Ok(CanonicalPlayerSnapshot {
        player_id,
        position,
        velocity,
        facing,
        forward,
        strafe,
        jump_pending,
        last_sequence,
        spawn_slot,
        combat: CanonicalPlayerCombat {
            level,
            health,
            target,
            auto_attack,
            swing_timer,
            combat_timer,
            calm_ticks,
            error_cooldown,
            intents,
            events,
        },
    })
}

fn decode_creature(
    payload: &[u8],
    offset: &mut usize,
) -> Result<CanonicalCreatureSnapshot, ProtocolError> {
    let creature_id = CreatureId::new(u32::from_be_bytes(take(payload, offset)?));
    let level = read_u8(payload, offset)?;
    let health = u32::from_be_bytes(take(payload, offset)?);
    let facing = u16::from_be_bytes(take(payload, offset)?);
    let position = read_vector(payload, offset)?;
    let velocity = read_vector(payload, offset)?;
    let life_code = read_u8(payload, offset)?;
    let died_at = u64::from_be_bytes(take(payload, offset)?);
    let life = match (life_code, died_at) {
        (ALIVE, 0) => CreatureLife::Alive,
        (CORPSE, died_at) => CreatureLife::Corpse { died_at },
        (DESPAWNED, died_at) => CreatureLife::Despawned { died_at },
        _ => return Err(ProtocolError::new("malformed creature life cycle")),
    };
    let ai_code = read_u8(payload, offset)?;
    let timer = u16::from_be_bytes(take(payload, offset)?);
    let has_destination = read_bool(payload, offset)?;
    let destination = [
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
    ];
    let destination = match (has_destination, destination) {
        (true, destination) => Some(destination),
        (false, [0, 0]) => None,
        (false, _) => return Err(ProtocolError::new("malformed creature destination")),
    };
    let ai = match (ai_code, timer, destination) {
        (IDLE, timer, destination) => CreatureAi::Idle { timer, destination },
        (ENGAGED, 0, None) => CreatureAi::Engaged,
        (EVADING, ticks, None) => CreatureAi::Evading { ticks },
        _ => return Err(ProtocolError::new("malformed creature AI state")),
    };
    let threat_count = decode_u8_count(
        payload,
        offset,
        MAX_THREAT_ENTRIES,
        "creature threat table exceeds its capacity",
    )?;
    let mut threat = Vec::with_capacity(threat_count);
    for _ in 0..threat_count {
        let entity: Option<EntityRef> = decode_entity_ref(payload, offset)?;
        let entity = entity.ok_or_else(|| ProtocolError::new("threat entry needs a unit"))?;
        threat.push(ThreatEntry {
            entity,
            threat: u32::from_be_bytes(take(payload, offset)?),
        });
    }
    let swing_timer = u16::from_be_bytes(take(payload, offset)?);
    let combat_timer = u16::from_be_bytes(take(payload, offset)?);
    let tapped = read_bool(payload, offset)?;
    let tapper = u32::from_be_bytes(take(payload, offset)?);
    let tapped_by = match (tapped, tapper) {
        (true, tapper) => Some(tapper),
        (false, 0) => None,
        (false, _) => return Err(ProtocolError::new("malformed creature tap")),
    };
    Ok(CanonicalCreatureSnapshot {
        creature_id,
        level,
        health,
        facing,
        position,
        velocity,
        life,
        ai,
        threat,
        swing_timer,
        combat_timer,
        tapped_by,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_snapshot;
    use mmorpg_core::{
        ErrorCode, NpcId, SNAPSHOT_SCHEMA_VERSION, ZoneDefinition, ZoneEvent, ZoneId,
        ZoneSimulation,
    };

    fn snapshot() -> CanonicalZoneSnapshot {
        let wolf = EntityRef::Creature(CreatureId::new(4));
        CanonicalZoneSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            content_revision: 3,
            content_fingerprint: 0x0123_4567_89ab_cdef,
            rng_state: u64::MAX,
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
                    combat: CanonicalPlayerCombat {
                        level: 2,
                        health: 17,
                        target: Some(wolf),
                        auto_attack: true,
                        swing_timer: 12,
                        combat_timer: 149,
                        calm_ticks: 0,
                        error_cooldown: 30,
                        intents: vec![
                            PlayerIntent::SelectTarget(Some(EntityRef::Npc(NpcId::new(2)))),
                            PlayerIntent::SelectTarget(None),
                            PlayerIntent::StartAttack,
                            PlayerIntent::StopAttack,
                            PlayerIntent::ReleaseSpirit,
                        ],
                        events: vec![
                            ZoneEvent::DamageTaken {
                                source: wolf,
                                target: EntityRef::Player(7),
                                amount: 4,
                                critical: true,
                            },
                            ZoneEvent::Error {
                                code: ErrorCode::YouAreDead,
                                target: None,
                            },
                        ],
                    },
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
                    combat: CanonicalPlayerCombat::default(),
                },
            ],
            creatures: vec![
                CanonicalCreatureSnapshot {
                    creature_id: CreatureId::new(4),
                    level: 1,
                    health: 30,
                    facing: 3,
                    position: [100, 45, 200],
                    velocity: [-19, -1, 0],
                    life: CreatureLife::Alive,
                    ai: CreatureAi::Engaged,
                    threat: vec![ThreatEntry {
                        entity: EntityRef::Player(7),
                        threat: 12,
                    }],
                    swing_timer: 5,
                    combat_timer: 150,
                    tapped_by: Some(7),
                },
                CanonicalCreatureSnapshot {
                    creature_id: CreatureId::new(5),
                    level: 1,
                    health: 0,
                    facing: 3,
                    position: [300, 45, 200],
                    velocity: [0; 3],
                    life: CreatureLife::Corpse { died_at: 90 },
                    ai: CreatureAi::Idle {
                        timer: 0,
                        destination: None,
                    },
                    threat: Vec::new(),
                    swing_timer: 0,
                    combat_timer: 0,
                    tapped_by: Some(8),
                },
                CanonicalCreatureSnapshot {
                    creature_id: CreatureId::new(6),
                    level: 1,
                    health: 30,
                    facing: 3,
                    position: [500, 45, 200],
                    velocity: [8, 0, 0],
                    life: CreatureLife::Alive,
                    ai: CreatureAi::Idle {
                        timer: 290,
                        destination: Some([-5, 900]),
                    },
                    threat: Vec::new(),
                    swing_timer: 0,
                    combat_timer: 0,
                    tapped_by: None,
                },
                CanonicalCreatureSnapshot {
                    creature_id: CreatureId::new(7),
                    level: 1,
                    health: 12,
                    facing: 3,
                    position: [700, 45, 200],
                    velocity: [0, 0, 24],
                    life: CreatureLife::Alive,
                    ai: CreatureAi::Evading { ticks: 44 },
                    threat: Vec::new(),
                    swing_timer: 0,
                    combat_timer: 0,
                    tapped_by: None,
                },
            ],
        }
    }

    #[test]
    fn canonical_snapshot_round_trip_preserves_continuation_state() {
        let snapshot = snapshot();
        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        assert_eq!(
            decode_snapshot(&encoded).unwrap_err().to_string(),
            "unexpected snapshot scope"
        );
        for length in 0..encoded.len() {
            assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_canonical_snapshot(&trailing).is_err());
        // Header 16, identity 24, then the first player record.
        let jump_flag = 16 + 24 + 2 + 32;
        assert_eq!(encoded[jump_flag], 1);
        for (offset, value, message) in [
            (jump_flag, 2, "jump flag must be 0 or 1"),
            (0, 4, "unsupported snapshot wire version"),
            (3, 4, "unsupported core snapshot schema version"),
            (40, 2, "snapshot exceeds configured zone player capacity"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
    }

    #[test]
    fn canonical_decoding_rejects_malformed_unit_state() {
        let encoded = encode_canonical_snapshot(&snapshot()).unwrap();
        // Player 7's auto-attack flag follows its target reference.
        let player = 16 + 24 + 2;
        let auto_attack = player + 39 + 1 + 4 + 5;
        let intents = auto_attack + 1 + 8;
        let mut cases = vec![
            (auto_attack, 2, "boolean field must be 0 or 1"),
            (intents, 17, "player has too many pending intents"),
            (intents + 1, 9, "malformed pending intent"),
        ];
        // The third intent (StartAttack) carries no target.
        cases.push((intents + 1 + 2 * 6 + 1, 2, "malformed pending intent"));
        for (offset, value, message) in cases {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
        let without_players = CanonicalZoneSnapshot {
            players: Vec::new(),
            ..snapshot()
        };
        let encoded = encode_canonical_snapshot(&without_players).unwrap();
        let creature = 16 + 24 + 2 + 2;
        let life = creature + 4 + 1 + 4 + 2 + 24;
        let ai = life + 9;
        for (offset, value, message) in [
            (life, 4, "malformed creature life cycle"),
            (life + 8, 1, "malformed creature life cycle"),
            (ai, 9, "malformed creature AI state"),
            (ai + 3, 2, "boolean field must be 0 or 1"),
            (ai + 4, 1, "malformed creature destination"),
            (ai + 12, 65, "creature threat table exceeds its capacity"),
            (ai + 13, 0, "an absent entity must have ID 0"),
            (ai + 12 + 1 + 9 + 4, 2, "boolean field must be 0 or 1"),
            (ai + 12 + 1 + 9 + 4, 0, "malformed creature tap"),
            (42, 5, "snapshot exceeds the creature spawn capacity"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
    }

    #[test]
    fn canonical_wire_restores_a_zone_and_its_content_identity() {
        let mut zone =
            ZoneSimulation::with_definition(ZoneId::new(1), ZoneDefinition::default()).unwrap();
        zone.add_player(1).unwrap();
        let state = zone.snapshot().unwrap();
        let encoded = encode_canonical_snapshot(&state).unwrap();
        let decoded = decode_canonical_snapshot(&encoded).unwrap();
        assert_eq!(decoded, state);
        let restored = ZoneSimulation::from_snapshot(decoded, zone.content().clone()).unwrap();
        assert_eq!(restored.snapshot().unwrap(), state);
        let other = ZoneSimulation::with_definition(
            ZoneId::new(1),
            ZoneDefinition::new(9, [0, -1, 0], Vec::new()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            ZoneSimulation::from_snapshot(state, other.content().clone())
                .err()
                .unwrap()
                .message(),
            "snapshot content identity does not match the supplied zone content"
        );
    }
}
