#[path = "support/visibility_oracle.rs"]
mod visibility_oracle;

use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, SNAPSHOT_SCHEMA_VERSION, ZoneCommand,
    ZoneDefinition, ZoneId, ZoneSimulation, outpost_definition,
};
use mmorpg_protocol::encode_snapshot;
use visibility_oracle::exhaustive_projection;

fn assert_wire_parity(zone: &ZoneSimulation) {
    let canonical = zone.snapshot().unwrap();
    for observer in &canonical.players {
        let expected = exhaustive_projection(&canonical, observer);
        let actual = zone.snapshot_for_player(observer.player_id).unwrap();
        assert_eq!(actual, expected, "observer {}", observer.player_id);
        assert_eq!(
            encode_snapshot(&actual).unwrap(),
            encode_snapshot(&expected).unwrap(),
            "observer {} at tick {}",
            observer.player_id,
            canonical.tick
        );
    }
}

fn zone_at(positions: &[[i32; 3]], definition: ZoneDefinition) -> ZoneSimulation {
    ZoneSimulation::from_snapshot(CanonicalZoneSnapshot {
        definition,
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        zone_id: ZoneId::new(9),
        tick: 0,
        players: positions
            .iter()
            .enumerate()
            .map(|(index, &position)| CanonicalPlayerSnapshot {
                player_id: u32::try_from(positions.len() - index).unwrap(),
                position,
                velocity: [0; 3],
                movement_x: 0,
                movement_z: 0,
                last_sequence: 3,
                spawn_slot: u16::try_from(index).unwrap(),
            })
            .collect(),
    })
    .unwrap()
}

#[test]
fn multi_tick_membership_transitions_match_exhaustive_wire_output() {
    let mut zone = zone_at(
        &[
            [1990, 50, 0],
            [-1999, 50, -1999],
            [6000, 50, 6000],
            [0, 50, 2000],
            [1200, 50, 1600],
        ],
        ZoneDefinition::default(),
    );
    assert_wire_parity(&zone);
    zone.advance_tick().unwrap(); // stationary
    assert_wire_parity(&zone);
    zone.apply_command(3, 4, ZoneCommand::SetMovement { x: 1, z: 0 })
        .unwrap(); // same cell
    zone.advance_tick().unwrap();
    assert_wire_parity(&zone);
    zone.apply_command(5, 4, ZoneCommand::SetMovement { x: 1, z: 0 })
        .unwrap();
    zone.apply_command(4, 4, ZoneCommand::SetMovement { x: -1, z: -1 })
        .unwrap(); // crosses both negative axes
    for tick in 0..18 {
        zone.advance_tick().unwrap();
        assert_wire_parity(&zone);
        if tick == 6 {
            let checkpoint = zone.snapshot().unwrap();
            zone = ZoneSimulation::from_snapshot(checkpoint.clone()).unwrap();
            assert_eq!(zone.snapshot().unwrap(), checkpoint);
            assert_wire_parity(&zone);
        }
    }
    assert!(zone.remove_player(2));
    assert_wire_parity(&zone);
    zone.add_player(2).unwrap();
    assert_wire_parity(&zone);
    zone.advance_tick().unwrap();
    assert_wire_parity(&zone);
}

#[test]
fn collision_corrections_and_failed_step_preserve_reference_parity() {
    let mut colliding = zone_at(&[[0, 50, 0], [2000, 50, 0]], outpost_definition());
    colliding
        .apply_command(2, 4, ZoneCommand::SetMovement { x: 0, z: -1 })
        .unwrap();
    for _ in 0..120 {
        colliding.advance_tick().unwrap();
        assert_wire_parity(&colliding);
    }
    assert!(
        colliding
            .snapshot()
            .unwrap()
            .players
            .iter()
            .find(|player| player.player_id == 2)
            .unwrap()
            .position[2]
            > -1440,
        "the shared physics collider must correct the nominal movement"
    );

    let mut edge = zone_at(
        &[[i32::MAX - 31, 50, 0], [i32::MIN + 31, 50, 0]],
        ZoneDefinition::default(),
    );
    edge.apply_command(2, 4, ZoneCommand::SetMovement { x: 1, z: 0 })
        .unwrap();
    edge.advance_tick().unwrap();
    edge.advance_tick().unwrap();
    let work = edge.interest_maintenance_stats();
    assert!(edge.advance_tick().is_err());
    assert_eq!(edge.interest_maintenance_stats(), work);
    assert_wire_parity(&edge);
}
