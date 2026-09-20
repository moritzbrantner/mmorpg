use mmorpg_client::presentation::Presentation;
use mmorpg_core::{
    PlayerSnapshot, SNAPSHOT_SCHEMA_VERSION, ZoneId, ZoneSnapshot, outpost_definition,
};
use std::time::{Duration, Instant};

fn snapshot(tick: u64, position: [i32; 3]) -> ZoneSnapshot {
    ZoneSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        zone_id: ZoneId::new(1),
        tick,
        content_revision: outpost_definition().revision(),
        acknowledged_sequence: 1,
        players: vec![PlayerSnapshot {
            player_id: 1,
            position,
            velocity: [12, 0, 0],
        }],
    }
}

#[test]
fn interpolation_preserves_large_ticks_holds_on_loss_and_ignores_old_packets() {
    let now = Instant::now();
    let mut presentation = Presentation::new(1, outpost_definition(), now);
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
    let definition = outpost_definition();
    let mut presentation = Presentation::new(1, definition.clone(), now);
    presentation
        .push(snapshot(1, [100, 50, -200]), now)
        .unwrap();
    let scene = presentation.scene(now);
    assert_eq!(scene.len(), definition.colliders().len() + 1);
    for (rendered, collider) in scene.iter().zip(definition.colliders()) {
        assert_eq!(
            rendered.position,
            collider.position.map(|value| value as f32 / 100.0)
        );
        assert_eq!(
            rendered.size,
            collider.half_extents.map(|value| value as f32 / 50.0)
        );
    }
    assert_eq!(scene.last().unwrap().position, [1.0, 0.5, -2.0]);
    assert_eq!(scene.last().unwrap().size, [0.6, 1.0, 0.6]);
}

#[test]
fn incompatible_content_zones_and_duplicate_entities_fail_closed() {
    let now = Instant::now();
    let mut presentation = Presentation::new(1, outpost_definition(), now);
    presentation.push(snapshot(1, [0; 3]), now).unwrap();
    let mut wrong_content = snapshot(2, [0; 3]);
    wrong_content.content_revision += 1;
    assert!(presentation.push(wrong_content, now).is_err());
    let mut wrong_zone = snapshot(2, [0; 3]);
    wrong_zone.zone_id = ZoneId::new(2);
    assert!(presentation.push(wrong_zone, now).is_err());
    let mut duplicate = snapshot(2, [0; 3]);
    duplicate.players.push(duplicate.players[0].clone());
    assert!(presentation.push(duplicate, now).is_err());
}
