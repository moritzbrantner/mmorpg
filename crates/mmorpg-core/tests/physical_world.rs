use mmorpg_core::{StaticCollider, ZoneCommand, ZoneDefinition, ZoneId, ZoneSimulation};

fn definition() -> ZoneDefinition {
    ZoneDefinition::new(
        7,
        [0, -1, 0],
        vec![
            StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [10_000, 50, 10_000],
            },
            StaticCollider {
                id: 2,
                position: [300, 100, 0],
                half_extents: [20, 100, 300],
            },
        ],
    )
    .unwrap()
}

#[test]
fn shared_physics_resolves_gravity_ground_and_wall_contact() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, ZoneCommand::SetMovement { x: 1, z: 0 })
        .unwrap();
    for _ in 0..100 {
        zone.advance_tick().unwrap();
    }
    let snapshot = zone.snapshot().unwrap();
    assert_eq!(snapshot.players[0].position, [250, 50, 0]);
    assert_eq!(snapshot.players[0].velocity, [0, 0, 0]);
    let projection = zone.snapshot_for_player(1).unwrap();
    assert_eq!(projection.content_revision, 7);
    assert_eq!(projection.acknowledged_sequence, 1);
}

#[test]
fn recovery_continues_airborne_motion_and_collision_identically() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition()).unwrap();
    zone.add_player(1).unwrap();
    let mut snapshot = zone.snapshot().unwrap();
    snapshot.players[0].position = [0, 400, 0];
    snapshot.players[0].velocity = [0, -7, 0];
    let mut original = ZoneSimulation::from_snapshot(snapshot).unwrap();
    original.advance_tick().unwrap();
    assert!(original.snapshot().unwrap().players[0].velocity[1] < -7);
    let mut restored = ZoneSimulation::from_snapshot(original.snapshot().unwrap()).unwrap();
    for _ in 0..50 {
        original.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(original.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert_eq!(restored.snapshot().unwrap().players[0].position[1], 50);
}

#[test]
fn content_rejects_ambiguous_ids_invalid_extents_and_overflow() {
    let collider = StaticCollider {
        id: 1,
        position: [0, 0, 0],
        half_extents: [1, 1, 1],
    };
    assert!(ZoneDefinition::new(1, [0; 3], vec![collider.clone(), collider.clone()]).is_err());
    assert!(
        ZoneDefinition::new(
            1,
            [0; 3],
            vec![StaticCollider {
                id: 1_000_000,
                ..collider.clone()
            }]
        )
        .is_err()
    );
    assert!(
        ZoneDefinition::new(
            1,
            [0; 3],
            vec![StaticCollider {
                half_extents: [-1, 1, 1],
                ..collider.clone()
            }]
        )
        .is_err()
    );
    assert!(
        ZoneDefinition::new(
            1,
            [0; 3],
            vec![StaticCollider {
                position: [i32::MAX, 0, 0],
                ..collider
            }]
        )
        .is_err()
    );
}

#[test]
fn exhausted_tick_does_not_move_physics() {
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, ZoneCommand::SetMovement { x: 1, z: 0 })
        .unwrap();
    let mut snapshot = zone.snapshot().unwrap();
    snapshot.tick = u64::MAX;
    let mut zone = ZoneSimulation::from_snapshot(snapshot.clone()).unwrap();
    assert!(zone.advance_tick().is_err());
    assert_eq!(zone.snapshot().unwrap(), snapshot);
}

#[test]
fn shared_outpost_ground_and_perimeter_stop_authoritative_movement() {
    let mut zone =
        ZoneSimulation::with_definition(ZoneId::new(1), mmorpg_core::outpost_definition()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, ZoneCommand::SetMovement { x: 0, z: -1 })
        .unwrap();
    for _ in 0..150 {
        zone.advance_tick().unwrap();
    }
    let player = &zone.snapshot().unwrap().players[0];
    assert_eq!(player.position, [0, 50, -1040]);
    assert_eq!(player.velocity, [0, 0, 0]);
}

#[test]
fn outpost_spawn_grid_is_supported_by_physics_at_configured_capacity() {
    let mut zone =
        ZoneSimulation::with_definition(ZoneId::new(1), mmorpg_core::outpost_definition()).unwrap();
    for id in 0..mmorpg_core::MAX_PLAYERS_PER_ZONE {
        zone.add_player(u32::try_from(id).unwrap()).unwrap();
    }
    let before = zone.snapshot().unwrap();
    zone.advance_tick().unwrap();
    let after = zone.snapshot().unwrap();
    assert_eq!(after.players.len(), before.players.len());
    for (before, after) in before.players.iter().zip(after.players) {
        assert_eq!(after.position, before.position);
        assert_eq!(after.velocity, [0, 0, 0]);
    }
}
