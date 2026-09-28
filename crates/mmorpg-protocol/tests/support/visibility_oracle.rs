//! Exhaustive XZ semantics. This development-only reference never uses index buckets.
use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, EntityKind, EntitySnapshot,
    INTEREST_RADIUS_UNITS, MAX_VISIBLE_ENTITIES, ZoneSnapshot,
};

/// Every player within the inclusive radius, the observer first and then by
/// `(squared XZ distance, kind, id)`, keeping at most the relevance cap.
pub fn exhaustive_projection(
    canonical: &CanonicalZoneSnapshot,
    observer: &CanonicalPlayerSnapshot,
) -> ZoneSnapshot {
    let distance_squared = |candidate: &CanonicalPlayerSnapshot| {
        let dx = i128::from(candidate.position[0]) - i128::from(observer.position[0]);
        let dz = i128::from(candidate.position[2]) - i128::from(observer.position[2]);
        dx * dx + dz * dz
    };
    let mut relevant: Vec<_> = canonical
        .players
        .iter()
        .filter(|candidate| distance_squared(candidate) <= i128::from(INTEREST_RADIUS_UNITS).pow(2))
        .collect();
    relevant.sort_by_key(|candidate| {
        (
            candidate.player_id != observer.player_id,
            distance_squared(candidate),
            candidate.player_id,
        )
    });
    ZoneSnapshot {
        content_revision: canonical.definition.revision(),
        acknowledged_sequence: observer.last_sequence,
        viewer_id: observer.player_id,
        schema_version: canonical.schema_version,
        zone_id: canonical.zone_id,
        tick: canonical.tick,
        entities: relevant
            .into_iter()
            .take(MAX_VISIBLE_ENTITIES)
            .map(|candidate| EntitySnapshot {
                kind: EntityKind::Player,
                id: candidate.player_id,
                position: candidate.position,
                velocity: candidate
                    .velocity
                    .map(|component| component.clamp(-128, 127) as i8),
                facing: candidate.facing,
            })
            .collect(),
    }
}
