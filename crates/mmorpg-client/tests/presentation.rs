use mmorpg_client::presentation::Presentation;
use mmorpg_core::{
    CreatureId, EntityFlags, EntityKind, EntityRef, EntitySnapshot, NpcId, SNAPSHOT_SCHEMA_VERSION,
    ViewerState, ZoneId, ZoneSnapshot, greyhaven_vale,
};
use mmorpg_scenery::greyhaven_vale_scenery;
use std::{
    f32::consts::{FRAC_PI_2, TAU},
    time::{Duration, Instant},
};

fn snapshot(tick: u64, position: [i32; 3]) -> ZoneSnapshot {
    facing_snapshot(tick, position, 0)
}

fn facing_snapshot(tick: u64, position: [i32; 3], facing: u16) -> ZoneSnapshot {
    ZoneSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        zone_id: ZoneId::new(1),
        tick,
        content_revision: greyhaven_vale::REVISION,
        acknowledged_sequence: 1,
        viewer_id: 1,
        viewer: ViewerState {
            health: 50,
            max_health: 50,
            level: 1,
            ..ViewerState::default()
        },
        target_of_target: None,
        events: Vec::new(),
        entities: vec![EntitySnapshot {
            kind: EntityKind::Player,
            id: 1,
            appearance: 0,
            position,
            velocity: [21, 0, 0],
            facing,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        }],
    }
}

fn presentation(now: Instant) -> Presentation {
    Presentation::new(1, greyhaven_vale_scenery(), greyhaven_vale::content(), now).unwrap()
}

// Samples below move along the Hollow Road (x = 0), where relief is flat.

#[test]
fn interpolation_preserves_large_ticks_holds_on_loss_and_ignores_old_packets() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation
        .push(snapshot(u64::MAX - 4, [0, 50, 0]), now)
        .unwrap();
    presentation
        .push(snapshot(u64::MAX, [0, 50, 400]), now)
        .unwrap();
    assert_eq!(presentation.camera_target(now), [0.0, 0.5, 2.0]);
    assert_eq!(
        presentation.camera_target(now + Duration::from_secs(1)),
        [0.0, 0.5, 4.0]
    );
    assert!(
        !presentation
            .push(snapshot(u64::MAX - 1, [0, 0, -100]), now)
            .unwrap()
    );
    assert_eq!(presentation.camera_target(now), [0.0, 0.5, 2.0]);
}

#[test]
fn units_render_on_the_shared_relief_with_entity_dimensions() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    // A meadow south of the outpost, off every road and structure.
    let scenery = greyhaven_vale_scenery();
    let (x, z, relief) = (2_000..11_000)
        .step_by(100)
        .map(|x| (x, 9_000, scenery.height_at(x, 9_000)))
        .find(|(_, _, relief)| *relief != 0)
        .expect("meadows roll");
    presentation
        .push(facing_snapshot(1, [x, 90, z], 16_384), now)
        .unwrap();
    let scene = presentation.scene(now);
    // Each player renders a body and a facing marker; scenery is static.
    let [body, nose] = scene[..] else {
        panic!("one body and one nose: {scene:?}");
    };
    let lifted = 0.9 + relief as f32 / 100.0;
    assert_eq!(body.position, [x as f32 / 100.0, lifted, z as f32 / 100.0]);
    assert_eq!(presentation.camera_target(now), body.position);
    assert_eq!(body.size, [0.6, 1.8, 0.6]);
    assert!(
        (body.yaw - FRAC_PI_2).abs() < 1e-6,
        "a quarter turn faces +X"
    );
    assert_eq!(nose.yaw, body.yaw);
    assert!(nose.position[0] > body.position[0] + body.size[0] / 2.0);
    assert!((nose.position[2] - body.position[2]).abs() < 1e-5);
    assert!(nose.position[1] > body.position[1], "at head height");
}

#[test]
fn facing_interpolates_along_the_shorter_arc() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation
        .push(facing_snapshot(10, [0, 90, 0], 65_000), now)
        .unwrap();
    presentation
        .push(facing_snapshot(14, [0, 90, 0], 500), now)
        .unwrap();
    // Two ticks of delay place the sample halfway: 65 000 + 518 steps.
    let halfway = presentation.players(now)[&1].yaw;
    let expected = 65_518.0 / 65_536.0 * TAU;
    assert!((halfway - expected).abs() < 1e-4, "{halfway} vs {expected}");
    let latest = presentation.players(now + Duration::from_secs(1))[&1].yaw;
    assert!((latest - 500.0 / 65_536.0 * TAU).abs() < 1e-4, "{latest}");
}

#[test]
fn incompatible_content_zones_and_duplicate_entities_fail_closed() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation.push(snapshot(1, [0; 3]), now).unwrap();
    let mut wrong_content = snapshot(2, [0; 3]);
    wrong_content.content_revision += 1;
    assert!(presentation.push(wrong_content, now).is_err());
    let mut wrong_zone = snapshot(2, [0; 3]);
    wrong_zone.zone_id = ZoneId::new(2);
    assert!(presentation.push(wrong_zone, now).is_err());
    let mut duplicate = snapshot(2, [0; 3]);
    duplicate.entities.push(duplicate.entities[0].clone());
    assert!(presentation.push(duplicate, now).is_err());
    let mut other_viewer = snapshot(2, [0; 3]);
    other_viewer.viewer_id = 2;
    assert!(presentation.push(other_viewer, now).is_err());
}

#[test]
fn a_resumed_connection_discards_old_interpolation_even_when_ticks_regress() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation.push(snapshot(100, [0, 50, 0]), now).unwrap();
    presentation.push(snapshot(104, [0, 50, 400]), now).unwrap();
    presentation.reset(now);
    assert!(presentation.players(now).is_empty());
    assert!(presentation.push(snapshot(2, [0, 50, 900]), now).unwrap());
    assert_eq!(presentation.camera_target(now), [0.0, 0.5, 9.0]);
}

fn creature(id: u32, template: u16, position: [i32; 3], flags: EntityFlags) -> EntitySnapshot {
    EntitySnapshot {
        kind: EntityKind::Creature,
        id,
        appearance: template,
        position,
        velocity: [0; 3],
        facing: 0,
        level: 2,
        health_percent: if flags.dead { 0 } else { 43 },
        flags,
    }
}

#[test]
fn creatures_and_npcs_render_by_size_disposition_and_state() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    let hostile = EntityFlags {
        hostile: true,
        attackable: true,
        ..EntityFlags::default()
    };
    let mut snapshot = snapshot(1, [0, 90, -2_000]);
    let wolf = EntityRef::Creature(CreatureId::new(108));
    snapshot.viewer.target = Some(wolf);
    snapshot.viewer.auto_attacking = true;
    snapshot.viewer.in_combat = true;
    snapshot.viewer.health = 38;
    snapshot.entities.extend([
        creature(108, 1, [0, 45, -1_800], hostile),
        creature(
            120,
            2,
            [300, 45, -2_000],
            EntityFlags {
                attackable: true,
                ..EntityFlags::default()
            },
        ),
        creature(
            109,
            1,
            [-300, 45, -2_000],
            EntityFlags {
                dead: true,
                hostile: true,
                ..EntityFlags::default()
            },
        ),
        EntitySnapshot {
            kind: EntityKind::Npc,
            id: 6,
            appearance: 6,
            position: [0, 90, -2_600],
            velocity: [0; 3],
            facing: 0,
            level: 10,
            health_percent: 100,
            flags: EntityFlags::default(),
        },
    ]);
    presentation.push(snapshot, now).unwrap();
    let scene = presentation.scene(now);
    // Player: body + nose; wolf: target marker + body + nose; boar: body +
    // nose; corpse: one flat body; NPC: body + nose.
    assert_eq!(scene.len(), 2 + 3 + 2 + 1 + 2);
    let at = |x: f32, z: f32| {
        scene
            .iter()
            .filter(|item| {
                (item.position[0] - x).abs() < 1e-4 && (item.position[2] - z).abs() < 1e-4
            })
            .collect::<Vec<_>>()
    };
    let wolf_boxes = at(0.0, -18.0);
    let marker = wolf_boxes[0];
    assert!(marker.size[1] < 0.05, "the target marker is flat");
    let wolf_body = wolf_boxes[1];
    assert_eq!(
        wolf_body.size,
        [0.8, 0.9, 0.8],
        "the template's collision box"
    );
    assert!(
        wolf_body.color[0] > wolf_body.color[1] + 0.3,
        "hostile is red-ish"
    );
    let boar_body = at(3.0, -20.0)[0];
    assert_eq!(boar_body.size, [0.9, 0.9, 0.9]);
    assert!(
        boar_body.color[0] > 0.6 && boar_body.color[1] > 0.5,
        "neutral is yellow-ish"
    );
    let corpse = at(-3.0, -20.0)[0];
    assert!(corpse.size[1] < 0.2, "a corpse lies flat");
    let guard = at(0.0, -26.0)[0];
    assert_eq!(guard.size, [0.6, 1.8, 0.6]);
    assert!(
        guard.color[1] > guard.color[0] + 0.2,
        "friendly is green-ish"
    );

    assert_eq!(
        presentation.status().unwrap(),
        "HP 38/50 · in combat · target Timber Wolf (L2, 43%) · attacking"
    );
    assert_eq!(
        presentation.unit_name(EntityRef::Npc(NpcId::new(6))),
        "Greyhaven Guard"
    );
    // Tab cycles through living attackable creatures, nearest first.
    assert_eq!(
        presentation.next_tab_target(),
        Some(EntityRef::Creature(CreatureId::new(120)))
    );
}

#[test]
fn scenery_and_content_of_different_revisions_are_refused() {
    let mut scenery = greyhaven_vale_scenery();
    scenery.content_revision += 1;
    assert!(Presentation::new(1, scenery, greyhaven_vale::content(), Instant::now()).is_err());
}
