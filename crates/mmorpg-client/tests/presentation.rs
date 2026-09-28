use mmorpg_client::presentation::Presentation;
use mmorpg_core::{
    EntityKind, EntitySnapshot, SNAPSHOT_SCHEMA_VERSION, ZoneId, ZoneSnapshot,
    greyhaven_vale_definition,
};
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
        content_revision: greyhaven_vale_definition().revision(),
        acknowledged_sequence: 1,
        viewer_id: 1,
        entities: vec![EntitySnapshot {
            kind: EntityKind::Player,
            id: 1,
            position,
            velocity: [21, 0, 0],
            facing,
        }],
    }
}

#[test]
fn interpolation_preserves_large_ticks_holds_on_loss_and_ignores_old_packets() {
    let now = Instant::now();
    let mut presentation = Presentation::new(1, greyhaven_vale_definition(), now);
    presentation
        .push(snapshot(u64::MAX - 4, [0, 50, 0]), now)
        .unwrap();
    presentation
        .push(snapshot(u64::MAX, [400, 50, 0]), now)
        .unwrap();
    assert_eq!(presentation.camera_target(now), [2.0, 0.5, 0.0]);
    assert_eq!(
        presentation.camera_target(now + Duration::from_secs(1)),
        [4.0, 0.5, 0.0]
    );
    assert!(
        !presentation
            .push(snapshot(u64::MAX - 1, [-100, 0, 0]), now)
            .unwrap()
    );
    assert_eq!(presentation.camera_target(now), [2.0, 0.5, 0.0]);
}

#[test]
fn rendering_uses_the_servers_collision_geometry_and_entity_dimensions() {
    let now = Instant::now();
    let definition = greyhaven_vale_definition();
    let mut presentation = Presentation::new(1, definition.clone(), now);
    presentation
        .push(facing_snapshot(1, [100, 90, -200], 16_384), now)
        .unwrap();
    let scene = presentation.scene(now);
    // Each player renders a body and a facing marker.
    assert_eq!(scene.len(), definition.colliders().len() + 2);
    for (rendered, collider) in scene.iter().zip(definition.colliders()) {
        assert_eq!(rendered.yaw, 0.0);
        assert_eq!(
            rendered.position,
            collider.position.map(|value| value as f32 / 100.0)
        );
        assert_eq!(
            rendered.size,
            collider.half_extents.map(|value| value as f32 / 50.0)
        );
    }
    let body = scene[definition.colliders().len()];
    assert_eq!(body.position, [1.0, 0.9, -2.0]);
    assert_eq!(body.size, [0.6, 1.8, 0.6]);
    assert!(
        (body.yaw - FRAC_PI_2).abs() < 1e-6,
        "a quarter turn faces +X"
    );
    let nose = scene[definition.colliders().len() + 1];
    assert_eq!(nose.yaw, body.yaw);
    assert!(nose.position[0] > body.position[0] + body.size[0] / 2.0);
    assert!((nose.position[2] - body.position[2]).abs() < 1e-5);
    assert!(nose.position[1] > body.position[1], "at head height");
}

#[test]
fn facing_interpolates_along_the_shorter_arc() {
    let now = Instant::now();
    let mut presentation = Presentation::new(1, greyhaven_vale_definition(), now);
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
    let mut presentation = Presentation::new(1, greyhaven_vale_definition(), now);
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
    let mut presentation = Presentation::new(1, greyhaven_vale_definition(), now);
    presentation.push(snapshot(100, [0, 50, 0]), now).unwrap();
    presentation.push(snapshot(104, [400, 50, 0]), now).unwrap();
    presentation.reset(now);
    assert!(presentation.players(now).is_empty());
    assert!(presentation.push(snapshot(2, [900, 50, 0]), now).unwrap());
    assert_eq!(presentation.camera_target(now), [9.0, 0.5, 0.0]);
}
