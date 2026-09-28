//! Exhaustive XZ semantics. This development-only reference never uses index buckets.
use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, EntityKind, EntitySnapshot,
    INTEREST_RADIUS_UNITS, ZoneSnapshot,
};

pub fn exhaustive_projection(
    canonical: &CanonicalZoneSnapshot,
    observer: &CanonicalPlayerSnapshot,
) -> ZoneSnapshot {
    ZoneSnapshot {
        content_revision: canonical.definition.revision(),
        acknowledged_sequence: observer.last_sequence,
        viewer_id: observer.player_id,
        schema_version: canonical.schema_version,
        zone_id: canonical.zone_id,
        tick: canonical.tick,
        entities: canonical
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
            .collect(),
    }
}
