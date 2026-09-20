//! Deterministic operation/byte counts, not a wall-clock throughput benchmark.
use std::error::Error;

use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, INTEREST_RADIUS_UNITS, MAX_PLAYERS_PER_ZONE,
    PlayerSnapshot, SNAPSHOT_SCHEMA_VERSION, ZoneDefinition, ZoneId, ZoneSimulation, ZoneSnapshot,
    outpost_definition,
};
use mmorpg_protocol::{SNAPSHOT_WIRE_VERSION, encode_canonical_snapshot, encode_snapshot};

fn main() -> Result<(), Box<dyn Error>> {
    for name in ["sparse-travel-512", "dense-hub-512", "outpost-spawn-512"] {
        measure(name)?;
    }
    Ok(())
}

fn measure(name: &str) -> Result<(), Box<dyn Error>> {
    let mut canonical = CanonicalZoneSnapshot {
        definition: ZoneDefinition::default(),
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        zone_id: ZoneId::new(1),
        tick: 0,
        players: Vec::new(),
    };
    for index in 0..MAX_PLAYERS_PER_ZONE {
        let value = i32::try_from(index)?;
        let position = match name {
            "sparse-travel-512" => [value * 6000, 50, 0],
            "dense-hub-512" => [(value % 23) * 64 - 704, 50, (value / 23) * 64 - 704],
            "outpost-spawn-512" => [(value % 32) * 200, 50, (value / 32) * 200],
            _ => return Err("unknown workload".into()),
        };
        canonical.players.push(CanonicalPlayerSnapshot {
            player_id: u32::try_from(index)? + 1,
            position,
            velocity: [0; 3],
            movement_x: 0,
            movement_z: 0,
            last_sequence: 0,
            spawn_slot: u16::try_from(index)?,
        });
    }
    if name == "outpost-spawn-512" {
        canonical.definition = outpost_definition();
    }
    let mut zone = ZoneSimulation::from_snapshot(canonical)?;
    // Includes a real shared-engine step and the ensuing index rebuild.
    zone.advance_tick()?;
    let canonical = zone.snapshot()?;
    let canonical_bytes = encode_canonical_snapshot(&canonical)?.len();
    let mut candidates = 0;
    let mut cells = 0;
    let mut visible = 0;
    let mut bytes = 0;
    let mut max_bytes = 0;
    for observer in &canonical.players {
        let projection = zone.project_for_player(observer.player_id)?;
        let baseline = exhaustive_projection(&canonical, observer);
        let encoded = encode_snapshot(&projection.snapshot)?;
        if encoded != encode_snapshot(&baseline)? {
            return Err(format!("{name}: projection differs for {}", observer.player_id).into());
        }
        candidates += projection.stats.candidates_tested;
        cells += projection.stats.cells_visited;
        visible += projection.snapshot.players.len();
        bytes += encoded.len();
        max_bytes = max_bytes.max(encoded.len());
    }
    let players = canonical.players.len();
    let baseline_tests = players * players;
    let revision = canonical.definition.revision();
    println!(
        "{{\"schema\":\"mmorpg.interest-workload/v1\",\"workload\":\"{name}\",\"core_schema\":{SNAPSHOT_SCHEMA_VERSION},\"wire_version\":{SNAPSHOT_WIRE_VERSION},\"content_revision\":{revision},\"radius_units\":{INTEREST_RADIUS_UNITS},\"players\":{players},\"index_entries_rebuilt\":{players},\"baseline_distance_tests\":{baseline_tests},\"indexed_distance_tests\":{candidates},\"cells_visited\":{cells},\"visible_records\":{visible},\"snapshot_payload_bytes\":{bytes},\"largest_snapshot_payload_bytes\":{max_bytes},\"canonical_payload_bytes\":{canonical_bytes},\"wire_parity\":true}}"
    );
    Ok(())
}

// The pre-index rule is deliberately exhaustive and independent of bucket logic.
fn exhaustive_projection(
    canonical: &CanonicalZoneSnapshot,
    observer: &CanonicalPlayerSnapshot,
) -> ZoneSnapshot {
    ZoneSnapshot {
        content_revision: canonical.definition.revision(),
        acknowledged_sequence: observer.last_sequence,
        schema_version: canonical.schema_version,
        zone_id: canonical.zone_id,
        tick: canonical.tick,
        players: canonical
            .players
            .iter()
            .filter(|candidate| {
                let dx = i128::from(candidate.position[0]) - i128::from(observer.position[0]);
                let dz = i128::from(candidate.position[2]) - i128::from(observer.position[2]);
                dx * dx + dz * dz <= i128::from(INTEREST_RADIUS_UNITS).pow(2)
            })
            .map(|candidate| PlayerSnapshot {
                player_id: candidate.player_id,
                position: candidate.position,
                velocity: candidate.velocity,
            })
            .collect(),
    }
}
