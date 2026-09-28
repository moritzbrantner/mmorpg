use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, EntityKind, EntitySnapshot,
    INTEREST_RADIUS_UNITS, MAX_PLAYERS_PER_ZONE, SNAPSHOT_SCHEMA_VERSION, ZoneCommand,
    ZoneDefinition, ZoneId, ZoneSimulation,
};

/// World-axis headings for forward movement: yaw 0 faces +Z, 90° faces +X.
const NORTH_EAST: u16 = 8_192;
const EAST: u16 = 16_384;
const SOUTH: u16 = 32_768;
const SOUTH_WEST: u16 = 40_960;
const WEST: u16 = 49_152;

fn run(facing: u16) -> ZoneCommand {
    ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing,
    }
}

fn zone_at(positions: &[[i32; 3]]) -> ZoneSimulation {
    ZoneSimulation::from_snapshot(CanonicalZoneSnapshot {
        definition: ZoneDefinition::default(),
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        zone_id: ZoneId::new(7),
        tick: 99,
        players: positions
            .iter()
            .enumerate()
            .map(|(slot, &position)| CanonicalPlayerSnapshot {
                // Reverse IDs deliberately: spatial traversal must not change wire order.
                player_id: u32::try_from(positions.len() - slot).unwrap(),
                position,
                velocity: [0; 3],
                facing: 0,
                forward: 0,
                strafe: 0,
                jump_pending: false,
                last_sequence: 3,
                spawn_slot: u16::try_from(slot).unwrap(),
            })
            .collect(),
    })
    .unwrap()
}

fn assert_matches_exhaustive(zone: &ZoneSimulation) {
    let canonical = zone.snapshot().unwrap();
    for observer in &canonical.players {
        let expected: Vec<_> = canonical
            .players
            .iter()
            .filter(|candidate| {
                let dx = i128::from(candidate.position[0]) - i128::from(observer.position[0]);
                let dz = i128::from(candidate.position[2]) - i128::from(observer.position[2]);
                dx * dx + dz * dz <= i128::from(INTEREST_RADIUS_UNITS).pow(2)
            })
            .map(|candidate| EntitySnapshot {
                kind: EntityKind::Player,
                id: candidate.player_id,
                position: candidate.position,
                velocity: candidate
                    .velocity
                    .map(|component| component.clamp(-32_768, 32_767) as i16),
                facing: candidate.facing,
            })
            .collect();
        let actual = zone.snapshot_for_player(observer.player_id).unwrap();
        assert_eq!(
            actual,
            mmorpg_core::ZoneSnapshot {
                content_revision: canonical.definition.revision(),
                acknowledged_sequence: observer.last_sequence,
                viewer_id: observer.player_id,
                schema_version: canonical.schema_version,
                zone_id: canonical.zone_id,
                tick: canonical.tick,
                entities: expected,
            },
            "observer {}",
            observer.player_id
        );
    }
    assert_eq!(zone.snapshot().unwrap(), canonical, "queries are read-only");
}

#[test]
fn visibility_preserves_inclusive_radius_height_independence_and_extreme_coordinates() {
    let zone = zone_at(&[
        [0, 50, 0],
        [2000, 50, 0],
        [2001, 50, 0],
        [-2000, 50, 0],
        [-2001, 50, 0],
        [1200, 50, 1600],
        [1201, 50, 1600],
        [-1, 50, -1],
        [0, 500_000, 0],
        [i32::MIN + 100, 50, i32::MIN + 100],
        [i32::MIN + 2100, 50, i32::MIN + 100],
        [i32::MAX - 100, 50, i32::MAX - 100],
        [i32::MAX - 2100, 50, i32::MAX - 100],
    ]);
    assert_matches_exhaustive(&zone);
    assert!(zone.snapshot_for_player(999).is_err());
}

#[test]
fn visibility_tracks_movement_admission_removal_and_recovery() {
    let mut zone = zone_at(&[[1990, 50, -2010], [3995, 50, -2010], [-1990, 50, 1990]]);
    zone.apply_command(3, 4, run(NORTH_EAST)).unwrap();
    zone.apply_command(1, 4, run(SOUTH_WEST)).unwrap();
    let mut restored = ZoneSimulation::from_snapshot(zone.snapshot().unwrap()).unwrap();
    for _ in 0..60 {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_matches_exhaustive(&zone);
        assert_matches_exhaustive(&restored);
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert!(zone.remove_player(2));
    assert!(!zone.remove_player(2));
    assert!(zone.snapshot_for_player(2).is_err());
    assert_matches_exhaustive(&zone);
    zone.add_player(2).unwrap();
    assert_matches_exhaustive(&zone);
    assert!(zone.add_player(2).is_err());
    assert_matches_exhaustive(&zone);
}

#[test]
fn full_capacity_projection_matches_exhaustive_for_reproducible_scattered_layout() {
    // A fixed integer sequence avoids ambient randomness and external test dependencies.
    let positions: Vec<_> = (0..MAX_PLAYERS_PER_ZONE)
        .map(|index| {
            let value = i32::try_from(index).unwrap();
            [
                (value * 7919) % 32000 - 16000,
                50,
                (value * 104729) % 32000 - 16000,
            ]
        })
        .collect();
    assert_matches_exhaustive(&zone_at(&positions));
}

#[test]
fn sparse_work_is_bounded_and_dense_visibility_is_never_truncated() {
    let sparse_positions: Vec<_> = (0..MAX_PLAYERS_PER_ZONE)
        .map(|index| [i32::try_from(index).unwrap() * 6000, 50, 0])
        .collect();
    let mut sparse = zone_at(&sparse_positions);
    assert!(sparse.add_player(9999).is_err());
    let mut sparse_candidates = 0;
    for player in sparse.snapshot().unwrap().players {
        let projection = sparse.project_for_player(player.player_id).unwrap();
        assert_eq!(projection.stats.cells_visited, 9);
        assert_eq!(projection.snapshot.entities.len(), 1);
        sparse_candidates += projection.stats.candidates_tested;
    }
    // The full-scan baseline does N*N distance tests for the same recipients.
    assert_eq!(sparse_candidates, MAX_PLAYERS_PER_ZONE);

    // 23 columns with non-overlapping 60-unit-wide bodies. Even the farthest
    // pair fits inside the radius; everyone must remain visible to everyone.
    let dense_positions: Vec<_> = (0..MAX_PLAYERS_PER_ZONE)
        .map(|index| {
            let index = i32::try_from(index).unwrap();
            [(index % 23) * 64 - 704, 50, (index / 23) * 64 - 704]
        })
        .collect();
    let dense = zone_at(&dense_positions);
    let mut dense_candidates = 0;
    for player in dense.snapshot().unwrap().players {
        let projection = dense.project_for_player(player.player_id).unwrap();
        assert_eq!(projection.stats.cells_visited, 9);
        assert_eq!(projection.snapshot.entities.len(), MAX_PLAYERS_PER_ZONE);
        dense_candidates += projection.stats.candidates_tested;
    }
    assert_eq!(dense_candidates, MAX_PLAYERS_PER_ZONE.pow(2));
    assert_matches_exhaustive(&dense);
}

#[test]
fn projection_tracks_collision_resolution_and_failed_physics_steps() {
    let mut zone =
        ZoneSimulation::with_definition(ZoneId::new(7), mmorpg_core::outpost_definition()).unwrap();
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    zone.apply_command(1, 1, run(SOUTH)).unwrap();
    for _ in 0..150 {
        zone.advance_tick().unwrap();
        assert_matches_exhaustive(&zone);
    }

    let mut edge = zone_at(&[[i32::MAX - 50, 50, 0], [i32::MIN + 31, 50, 0]]);
    edge.apply_command(2, 4, run(EAST)).unwrap();
    edge.advance_tick().unwrap();
    edge.advance_tick().unwrap();
    let before_failure = edge.interest_maintenance_stats();
    assert!(edge.advance_tick().is_err());
    assert_eq!(edge.current_tick(), 101);
    assert_eq!(edge.interest_maintenance_stats(), before_failure);
    assert_matches_exhaustive(&edge);
}

#[test]
fn crossing_into_a_previously_unqueried_cell_becomes_visible_on_the_next_tick() {
    let mut zone = zone_at(&[
        [1999, 50, 0],
        [4001, 50, 0],
        [-1999, 50, 5000],
        [-4001, 50, 5000],
    ]);
    zone.apply_command(3, 4, run(WEST)).unwrap();
    zone.apply_command(1, 4, run(EAST)).unwrap();
    let mut recovered = ZoneSimulation::from_snapshot(zone.snapshot().unwrap()).unwrap();
    for simulation in [&mut zone, &mut recovered] {
        for observer in [4, 2] {
            assert_eq!(
                simulation
                    .snapshot_for_player(observer)
                    .unwrap()
                    .entities
                    .len(),
                1
            );
        }
        simulation.advance_tick().unwrap();
        for observer in [4, 2] {
            assert_eq!(
                simulation
                    .snapshot_for_player(observer)
                    .unwrap()
                    .entities
                    .len(),
                2
            );
        }
        assert_matches_exhaustive(simulation);
    }
}

#[test]
fn retained_memberships_write_only_for_crossings_across_deterministic_ticks() {
    let mut zone = zone_at(&[
        [1990, 50, 0],
        [-1999, 50, -1999],
        [6000, 50, 6000],
        [12_000, 50, 0],
    ]);
    let recovered = ZoneSimulation::from_snapshot(zone.snapshot().unwrap()).unwrap();
    assert_eq!(recovered.interest_maintenance_stats().full_rebuilds, 1);
    let initial = zone.interest_maintenance_stats();
    zone.advance_tick().unwrap();
    assert_matches_exhaustive(&zone);
    let stationary = zone.interest_maintenance_stats();
    assert_eq!(stationary.full_rebuilds, initial.full_rebuilds);
    assert_eq!(stationary.bucket_inserts, initial.bucket_inserts);
    assert_eq!(stationary.bucket_removes, initial.bucket_removes);
    assert_eq!(stationary.bucket_moves, initial.bucket_moves);
    assert_eq!(stationary.players_inspected - initial.players_inspected, 4);

    zone.apply_command(1, 4, run(EAST)).unwrap();
    zone.advance_tick().unwrap();
    assert_matches_exhaustive(&zone);
    let same_cell = zone.interest_maintenance_stats();
    assert_eq!(same_cell.bucket_inserts, stationary.bucket_inserts);
    assert_eq!(same_cell.bucket_removes, stationary.bucket_removes);
    assert_eq!(same_cell.bucket_moves, stationary.bucket_moves);

    zone.apply_command(2, 4, run(EAST)).unwrap();
    zone.apply_command(3, 4, run(SOUTH_WEST)).unwrap();
    zone.apply_command(4, 4, run(EAST)).unwrap();
    zone.advance_tick().unwrap();
    assert_matches_exhaustive(&zone);
    let crossed = zone.interest_maintenance_stats();
    assert_eq!(crossed.full_rebuilds, 1);
    assert_eq!(crossed.bucket_moves - same_cell.bucket_moves, 2);
    assert_eq!(crossed.bucket_inserts - same_cell.bucket_inserts, 2);
    assert_eq!(crossed.bucket_removes - same_cell.bucket_removes, 2);
    assert_eq!(crossed.players_inspected - same_cell.players_inspected, 4);

    let checkpoint = zone.snapshot().unwrap();
    let mut recovered = ZoneSimulation::from_snapshot(checkpoint.clone()).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), checkpoint);
    assert_matches_exhaustive(&recovered);
    for _ in 0..10 {
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), recovered.snapshot().unwrap());
        assert_matches_exhaustive(&zone);
        assert_matches_exhaustive(&recovered);
    }
    let before_removal = zone.interest_maintenance_stats();
    assert!(zone.remove_player(3));
    zone.add_player(3).unwrap();
    let after_admission = zone.interest_maintenance_stats();
    assert_eq!(
        after_admission.bucket_removes - before_removal.bucket_removes,
        1
    );
    assert_eq!(
        after_admission.bucket_inserts - before_removal.bucket_inserts,
        1
    );
    assert_matches_exhaustive(&zone);
}
