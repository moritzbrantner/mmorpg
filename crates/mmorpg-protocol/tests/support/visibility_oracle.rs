//! Exhaustive XZ semantics. This development-only reference never uses index buckets.
use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, INTEREST_RADIUS_UNITS, PlayerSnapshot,
    ZoneSnapshot,
};

pub fn exhaustive_projection(
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
