//! Player-visible scope: one player's projection, packed into a single
//! datagram by fixed section priority (header, self, target details,
//! events, then entities by relevance until the budget is used).

use mmorpg_core::{
    EntityFlags, EntityKind, EntitySnapshot, MAX_EVENTS_PER_PLAYER, MAX_VISIBLE_ENTITIES,
    ViewerState, ZoneSnapshot,
};

use crate::wire::{
    COMMON_HEADER_BYTES, ENTITY_REF_BYTES, PLAYER_SNAPSHOT_SCOPE, decode_common_header,
    decode_entity_kind, decode_entity_ref, decode_event, decode_u8_count, decode_u16_count,
    encode_common_header, encode_entity_ref, encode_event, encode_u8_count, ensure_fully_consumed,
    entity_kind_code, flag_byte, read_flags, read_u8, take,
};
use crate::{MAX_PLAYER_PROJECTION_BYTES, ProtocolError};

/// Content revision, acknowledged sequence and viewer ID after the prefix.
const HEADER_BYTES: usize = COMMON_HEADER_BYTES + 8 + 4 + 4;
/// Health, maximum health, level, flags and the viewer's target.
const SELF_BYTES: usize = 4 + 4 + 1 + 1 + ENTITY_REF_BYTES;
/// The target's own target.
const TARGET_BYTES: usize = ENTITY_REF_BYTES;
/// Every section's fixed part: header, self, target, event count (`u8`)
/// and entity count (`u16`).
pub const PLAYER_SNAPSHOT_FIXED_BYTES: usize = HEADER_BYTES + SELF_BYTES + TARGET_BYTES + 1 + 2;
/// Kind, flags, source and target references, `u16` amount.
pub const EVENT_RECORD_BYTES: usize = 2 + 2 * ENTITY_REF_BYTES + 2;
/// Kind, ID, appearance, `3 × i16` position, `3 × i8` velocity, `u16`
/// facing, level, health percent and flags.
pub const ENTITY_RECORD_BYTES: usize = 1 + 4 + 2 + 6 + 3 + 2 + 1 + 1 + 1;
/// Records that fit the budget without events: (1,077 − 55) / 21 = 48.
pub const MAX_WIRE_ENTITIES: usize =
    (MAX_PLAYER_PROJECTION_BYTES - PLAYER_SNAPSHOT_FIXED_BYTES) / ENTITY_RECORD_BYTES;
const MAX_EVENT_SECTION_BYTES: usize = MAX_EVENTS_PER_PLAYER * EVENT_RECORD_BYTES;

// With a full event section, the viewer and its target always fit:
// 55 + 16 × 14 + 2 × 21 = 321 ≤ 1,077 bytes.
const _: () = assert!(
    PLAYER_SNAPSHOT_FIXED_BYTES + MAX_EVENT_SECTION_BYTES + 2 * ENTITY_RECORD_BYTES
        <= MAX_PLAYER_PROJECTION_BYTES
);
// Budget packing, not the relevance cap, bounds the entity count.
const _: () = assert!(MAX_WIRE_ENTITIES <= MAX_VISIBLE_ENTITIES);

/// A player projection packed within [`MAX_PLAYER_PROJECTION_BYTES`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedSnapshot {
    pub payload: Vec<u8>,
    /// Leading entities of the projection that fit; the rest were omitted.
    pub packed_entities: usize,
}

/// Budget-driven packing: writes the header, the viewer's own state, its
/// target's target and this tick's events, then entities in the projection's
/// priority order until the next record would exceed
/// [`MAX_PLAYER_PROJECTION_BYTES`]. The viewer and its target lead the
/// entities and always fit. Hosts publish this; positions outside the `i16`
/// range fail closed rather than being clamped.
pub fn pack_snapshot(snapshot: &ZoneSnapshot) -> Result<PackedSnapshot, ProtocolError> {
    if snapshot
        .entities
        .first()
        .is_none_or(|entity| (entity.kind, entity.id) != (EntityKind::Player, snapshot.viewer_id))
    {
        return Err(ProtocolError::new(
            "player projection must start with the viewer",
        ));
    }
    let event_count = encode_u8_count(
        snapshot.events.len(),
        MAX_EVENTS_PER_PLAYER,
        "player projection has too many events",
    )?;
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

    let viewer = &snapshot.viewer;
    payload.extend_from_slice(&viewer.health.to_be_bytes());
    payload.extend_from_slice(&viewer.max_health.to_be_bytes());
    payload.push(viewer.level);
    payload.push(flag_byte(&[
        viewer.dead,
        viewer.in_combat,
        viewer.auto_attacking,
    ]));
    encode_entity_ref(&mut payload, viewer.target);
    encode_entity_ref(&mut payload, snapshot.target_of_target);

    payload.push(event_count);
    for event in &snapshot.events {
        encode_event(&mut payload, event);
    }

    let count_offset = payload.len();
    payload.extend_from_slice(&0_u16.to_be_bytes());
    let mut packed_entities = 0_u16;
    for entity in snapshot.entities.iter().take(MAX_WIRE_ENTITIES) {
        if payload.len() + ENTITY_RECORD_BYTES > MAX_PLAYER_PROJECTION_BYTES {
            break;
        }
        encode_entity(&mut payload, entity)?;
        packed_entities += 1;
    }
    payload[count_offset..count_offset + 2].copy_from_slice(&packed_entities.to_be_bytes());
    Ok(PackedSnapshot {
        payload,
        packed_entities: usize::from(packed_entities),
    })
}

/// Encodes a complete player projection. Fails closed, never truncating
/// silently, when any entity would not fit [`MAX_PLAYER_PROJECTION_BYTES`].
/// Fixtures and tests use it; hosts publish with [`pack_snapshot`].
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
    payload.extend_from_slice(&entity.appearance.to_be_bytes());
    for component in entity.position {
        let component = i16::try_from(component)
            .map_err(|_| ProtocolError::new("entity position is outside the compact wire range"))?;
        payload.extend_from_slice(&component.to_be_bytes());
    }
    for component in entity.velocity {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.extend_from_slice(&entity.facing.to_be_bytes());
    payload.push(entity.level);
    if entity.health_percent > 100 {
        return Err(ProtocolError::new("health percent exceeds 100"));
    }
    payload.push(entity.health_percent);
    let flags = entity.flags;
    payload.push(flag_byte(&[
        flags.dead,
        flags.in_combat,
        flags.hostile,
        flags.attackable,
        flags.tapped_by_other,
        flags.evading,
        flags.targets_viewer,
    ]));
    Ok(())
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

    let health = u32::from_be_bytes(take(payload, &mut offset)?);
    let max_health = u32::from_be_bytes(take(payload, &mut offset)?);
    let level = read_u8(payload, &mut offset)?;
    let [dead, in_combat, auto_attacking] = read_flags(read_u8(payload, &mut offset)?)?;
    let target = decode_entity_ref(payload, &mut offset)?;
    if level == 0 || health > max_health || dead != (health == 0) {
        return Err(ProtocolError::new("viewer state is inconsistent"));
    }
    let target_of_target = decode_entity_ref(payload, &mut offset)?;

    let event_count = decode_u8_count(
        payload,
        &mut offset,
        MAX_EVENTS_PER_PLAYER,
        "snapshot exceeds the per-player event capacity",
    )?;
    let mut events = Vec::with_capacity(event_count);
    for _ in 0..event_count {
        events.push(decode_event(payload, &mut offset)?);
    }

    let count = decode_u16_count(
        payload,
        &mut offset,
        MAX_WIRE_ENTITIES,
        "snapshot exceeds configured visible entity capacity",
    )?;
    let mut entities = Vec::with_capacity(count);
    for _ in 0..count {
        entities.push(decode_entity(payload, &mut offset)?);
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
        viewer: ViewerState {
            health,
            max_health,
            level,
            dead,
            in_combat,
            auto_attacking,
            target,
        },
        target_of_target,
        events,
        entities,
    })
}

fn decode_entity(payload: &[u8], offset: &mut usize) -> Result<EntitySnapshot, ProtocolError> {
    let kind = decode_entity_kind(read_u8(payload, offset)?)?;
    let id = u32::from_be_bytes(take(payload, offset)?);
    let appearance = u16::from_be_bytes(take(payload, offset)?);
    let position = [
        i16::from_be_bytes(take(payload, offset)?),
        i16::from_be_bytes(take(payload, offset)?),
        i16::from_be_bytes(take(payload, offset)?),
    ]
    .map(i32::from);
    let velocity = [
        i8::from_be_bytes(take(payload, offset)?),
        i8::from_be_bytes(take(payload, offset)?),
        i8::from_be_bytes(take(payload, offset)?),
    ];
    let facing = u16::from_be_bytes(take(payload, offset)?);
    let level = read_u8(payload, offset)?;
    let health_percent = read_u8(payload, offset)?;
    if health_percent > 100 {
        return Err(ProtocolError::new("health percent exceeds 100"));
    }
    let [
        dead,
        in_combat,
        hostile,
        attackable,
        tapped_by_other,
        evading,
        targets_viewer,
    ] = read_flags(read_u8(payload, offset)?)?;
    Ok(EntitySnapshot {
        kind,
        id,
        appearance,
        position,
        velocity,
        facing,
        level,
        health_percent,
        flags: EntityFlags {
            dead,
            in_combat,
            hostile,
            attackable,
            tapped_by_other,
            evading,
            targets_viewer,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DATAGRAM_SAFETY_MARGIN_BYTES, MEASURED_MIN_DATAGRAM_BYTES, SESSION_SNAPSHOT_HEADER_BYTES,
    };
    use mmorpg_core::{
        CreatureId, EntityRef, ErrorCode, NpcId, SNAPSHOT_SCHEMA_VERSION, ZoneEvent, ZoneId,
    };

    const VIEWER: EntityRef = EntityRef::Player(7);
    const WOLF: EntityRef = EntityRef::Creature(CreatureId::new(108));

    fn player(id: u32, position: [i32; 3], velocity: [i8; 3], facing: u16) -> EntitySnapshot {
        EntitySnapshot {
            kind: EntityKind::Player,
            id,
            appearance: 0,
            position,
            velocity,
            facing,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        }
    }

    /// The shared Rust/browser golden projection: a viewer fighting a
    /// tapped wolf next to an NPC, with one event of every kind.
    fn fixture_snapshot() -> ZoneSnapshot {
        ZoneSnapshot {
            content_revision: 3,
            acknowledged_sequence: 81,
            viewer_id: 7,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            viewer: ViewerState {
                health: 38,
                max_health: 50,
                level: 1,
                dead: false,
                in_combat: true,
                auto_attacking: true,
                target: Some(WOLF),
            },
            target_of_target: Some(VIEWER),
            events: vec![
                ZoneEvent::DamageDealt {
                    source: VIEWER,
                    target: WOLF,
                    amount: 7,
                    critical: true,
                },
                ZoneEvent::DamageTaken {
                    source: WOLF,
                    target: VIEWER,
                    amount: 3,
                    critical: false,
                },
                ZoneEvent::Miss {
                    source: VIEWER,
                    target: WOLF,
                },
                ZoneEvent::Evade {
                    source: VIEWER,
                    target: WOLF,
                },
                ZoneEvent::Died {
                    entity: WOLF,
                    killer: Some(VIEWER),
                },
                ZoneEvent::Error {
                    code: ErrorCode::OutOfRange,
                    target: Some(WOLF),
                },
                ZoneEvent::Error {
                    code: ErrorCode::NoTarget,
                    target: None,
                },
            ],
            entities: vec![
                EntitySnapshot {
                    health_percent: 76,
                    flags: EntityFlags {
                        in_combat: true,
                        ..EntityFlags::default()
                    },
                    ..player(7, [10, 90, -30], [1, -2, 3], 16_384)
                },
                EntitySnapshot {
                    kind: EntityKind::Creature,
                    id: 108,
                    appearance: 1,
                    position: [-845, 45, 2_500],
                    velocity: [-13, 0, i8::MIN],
                    facing: 49_152,
                    level: 2,
                    health_percent: 43,
                    flags: EntityFlags {
                        dead: false,
                        in_combat: true,
                        hostile: true,
                        attackable: true,
                        tapped_by_other: false,
                        evading: false,
                        targets_viewer: true,
                    },
                },
                EntitySnapshot {
                    kind: EntityKind::Npc,
                    id: 5,
                    appearance: 5,
                    position: [-1_650, 90, 4_550],
                    velocity: [0; 3],
                    facing: 49_152,
                    level: 10,
                    health_percent: 100,
                    flags: EntityFlags::default(),
                },
                EntitySnapshot {
                    kind: EntityKind::Creature,
                    id: 109,
                    appearance: 1,
                    position: [-900, 45, 2_600],
                    velocity: [0; 3],
                    facing: 0,
                    level: 1,
                    health_percent: 0,
                    flags: EntityFlags {
                        dead: true,
                        hostile: true,
                        tapped_by_other: true,
                        ..EntityFlags::default()
                    },
                },
            ],
        }
    }

    fn fixture_bytes() -> Vec<u8> {
        let fixture = include_str!("../../../fixtures/protocol/player-snapshot-v5.hex").trim();
        (0..fixture.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&fixture[offset..offset + 2], 16).unwrap())
            .collect()
    }

    /// Byte offsets of the fixture's sections.
    const EVENTS: usize = 52;
    const ENTITY_COUNT: usize = EVENTS + 1 + 7 * EVENT_RECORD_BYTES;
    const FIRST_ENTITY: usize = ENTITY_COUNT + 2;

    #[test]
    fn player_snapshot_matches_the_shared_golden_fixture() {
        let snapshot = fixture_snapshot();
        let encoded = encode_snapshot(&snapshot).unwrap();
        if std::env::var_os("MMORPG_PRINT_FIXTURE").is_some() {
            println!(
                "{}",
                encoded
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
        }
        assert_eq!(
            encoded.len(),
            PLAYER_SNAPSHOT_FIXED_BYTES + 7 * EVENT_RECORD_BYTES + 4 * ENTITY_RECORD_BYTES
        );
        assert_eq!(encoded, fixture_bytes());
        assert_eq!(decode_snapshot(&encoded).unwrap(), snapshot);
        assert_eq!(encoded[EVENTS], 7);
        assert_eq!(encoded[FIRST_ENTITY], 1, "the viewer's record leads");
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
        let wolf_record = FIRST_ENTITY + ENTITY_RECORD_BYTES;
        for (offset, value, message) in [
            (0, 4, "unsupported snapshot wire version"),
            (1, 1, "unexpected snapshot scope"),
            (3, 4, "unsupported core snapshot schema version"),
            (41, 0b1000, "reserved flag bits are set"),
            (41, 0b111, "viewer state is inconsistent"),
            (42, 9, "unknown entity kind"),
            (47, 0, "an absent entity must have ID 0"),
            (EVENTS, 17, "snapshot exceeds the per-player event capacity"),
            (EVENTS + 1, 9, "unknown event kind"),
            (EVENTS + 2, 2, "event flags are invalid"),
            (
                EVENTS + 1 + 2 * EVENT_RECORD_BYTES + 1,
                1,
                "event flags are invalid",
            ),
            (
                EVENTS + 1 + 6 * EVENT_RECORD_BYTES + 13,
                99,
                "unknown error code",
            ),
            (FIRST_ENTITY, 0, "unknown entity kind"),
            (FIRST_ENTITY, 4, "unknown entity kind"),
            (wolf_record + 19, 101, "health percent exceeds 100"),
            (wolf_record + 20, 0x80, "reserved flag bits are set"),
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
        swapped[FIRST_ENTITY..].rotate_left(ENTITY_RECORD_BYTES);
        assert_eq!(
            decode_snapshot(&swapped).unwrap_err().to_string(),
            "player projection must start with the viewer"
        );
        let mut fewer = encoded.clone();
        fewer[ENTITY_COUNT..FIRST_ENTITY].copy_from_slice(&3_u16.to_be_bytes());
        assert!(decode_snapshot(&fewer).is_err(), "count must match records");
        let mut more = encoded;
        more[ENTITY_COUNT..FIRST_ENTITY].copy_from_slice(&5_u16.to_be_bytes());
        assert!(decode_snapshot(&more).is_err(), "count must match records");
    }

    #[test]
    fn snapshot_decoder_rejects_counts_and_sizes_above_the_budget_before_allocation() {
        let mut encoded = encode_snapshot(&ZoneSnapshot {
            events: Vec::new(),
            entities: vec![fixture_snapshot().entities[0].clone()],
            ..fixture_snapshot()
        })
        .unwrap();
        let excessive_count = u16::try_from(MAX_WIRE_ENTITIES + 1).unwrap();
        encoded[EVENTS + 1..EVENTS + 3].copy_from_slice(&excessive_count.to_be_bytes());
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

    /// Every section at its extreme: 16 events and the whole relevance cap
    /// with extreme field values. Packing keeps the viewer, its target and as
    /// many nearest units as fit, within one datagram.
    #[test]
    fn the_largest_projection_packs_into_the_datagram_budget() {
        assert_eq!(MAX_PLAYER_PROJECTION_BYTES, 1_161 - 20 - 64);
        assert_eq!(
            (
                PLAYER_SNAPSHOT_FIXED_BYTES,
                EVENT_RECORD_BYTES,
                ENTITY_RECORD_BYTES
            ),
            (55, 14, 21)
        );
        assert_eq!(MAX_WIRE_ENTITIES, 48);
        let viewer_id = u32::MAX;
        let target = EntityRef::Creature(CreatureId::new(u32::MAX));
        let entities: Vec<_> = (0..MAX_VISIBLE_ENTITIES)
            .map(|index| {
                let (low, high) = if index % 2 == 0 {
                    (i16::MIN, i16::MAX)
                } else {
                    (i16::MAX, i16::MIN)
                };
                let (kind, id) = match index {
                    0 => (EntityKind::Player, viewer_id),
                    1 => (EntityKind::Creature, u32::MAX),
                    _ => (EntityKind::Npc, u32::try_from(index).unwrap()),
                };
                EntitySnapshot {
                    kind,
                    id,
                    appearance: u16::MAX,
                    position: [low, high, low].map(i32::from),
                    velocity: [i8::MIN, i8::MAX, i8::MIN],
                    facing: u16::MAX,
                    level: u8::MAX,
                    health_percent: 100,
                    flags: EntityFlags {
                        dead: false,
                        in_combat: true,
                        hostile: true,
                        attackable: true,
                        tapped_by_other: true,
                        evading: true,
                        targets_viewer: true,
                    },
                }
            })
            .collect();
        let event = ZoneEvent::DamageTaken {
            source: target,
            target: EntityRef::Player(viewer_id),
            amount: u16::MAX,
            critical: true,
        };
        let snapshot = ZoneSnapshot {
            content_revision: u64::MAX,
            acknowledged_sequence: u32::MAX,
            viewer_id,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(u32::MAX),
            tick: u64::MAX,
            viewer: ViewerState {
                health: u32::MAX,
                max_health: u32::MAX,
                level: u8::MAX,
                dead: false,
                in_combat: true,
                auto_attacking: true,
                target: Some(target),
            },
            target_of_target: Some(EntityRef::Npc(NpcId::new(u32::MAX))),
            events: vec![event; MAX_EVENTS_PER_PLAYER],
            entities,
        };
        let packed = pack_snapshot(&snapshot).unwrap();
        // (1,077 − 55 − 16 × 14) / 21 = 38 records fit beside a full event section.
        assert_eq!(packed.packed_entities, 38);
        assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
        assert!(
            packed.payload.len() + SESSION_SNAPSHOT_HEADER_BYTES + DATAGRAM_SAFETY_MARGIN_BYTES
                <= MEASURED_MIN_DATAGRAM_BYTES
        );
        let decoded = decode_snapshot(&packed.payload).unwrap();
        assert_eq!(decoded.entities[..], snapshot.entities[..38]);
        assert_eq!(
            decoded.entities[1].entity(),
            target,
            "the target always fits"
        );
        assert_eq!(decoded.events, snapshot.events);
        assert_eq!(
            encode_snapshot(&snapshot).unwrap_err().to_string(),
            "player projection exceeds the byte budget"
        );
        // Without events the budget holds 48 records.
        let quiet = ZoneSnapshot {
            events: Vec::new(),
            ..snapshot
        };
        let packed = pack_snapshot(&quiet).unwrap();
        assert_eq!(packed.packed_entities, MAX_WIRE_ENTITIES);
        assert_eq!(
            packed.payload.len(),
            PLAYER_SNAPSHOT_FIXED_BYTES + MAX_WIRE_ENTITIES * ENTITY_RECORD_BYTES
        );
    }

    #[test]
    fn snapshot_encoding_fails_closed_on_missing_viewer_wide_positions_or_excess_events() {
        let mut no_viewer = fixture_snapshot();
        no_viewer.entities.clear();
        let mut viewer_second = fixture_snapshot();
        viewer_second.entities.swap(0, 1);
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
        let mut chatty = fixture_snapshot();
        chatty.events = vec![chatty.events[0]; MAX_EVENTS_PER_PLAYER + 1];
        assert_eq!(
            encode_snapshot(&chatty).unwrap_err().to_string(),
            "player projection has too many events"
        );
    }
}
