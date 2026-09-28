//! Immutable collision content, shared by simulation construction and scene export.
//! Render meshes/materials are presentation data; these boxes define physical truth.

use crate::{MAX_PLAYERS_PER_ZONE, PLAYER_HALF_EXTENTS_UNITS, ZoneError};

pub const MAX_STATIC_COLLIDERS: usize = 1_024;
/// One render metre corresponds to 100 integer simulation units; Y points up.
pub const UNITS_PER_METRE: i32 = 100;
/// Every collider bound and spawn position lies within `±` this many units on
/// each axis. Player projections carry positions as `i16`; walls inside this
/// range keep every reachable position representable with headroom.
pub const MAX_CONTENT_COORDINATE_UNITS: i32 = 32_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollider {
    pub id: u32,
    pub position: [i32; 3],
    pub half_extents: [i32; 3],
}

/// Row-major spawn slots on flat ground. Slot `s` places a character's feet at
/// `(origin[0] + (s % columns) × spacing, 0, origin[1] + (s / columns) × spacing)`.
/// A zone definition proves every slot below `MAX_PLAYERS_PER_ZONE` is clear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpawnGrid {
    /// XZ feet position of slot 0.
    pub origin: [i32; 2],
    pub columns: u16,
    pub spacing: i32,
}

impl SpawnGrid {
    /// The grid of content without an authored spawn area: 32 columns, 2 m
    /// apart, starting at the origin.
    pub const DEFAULT: Self = Self {
        origin: [0, 0],
        columns: 32,
        spacing: 200,
    };

    /// Feet position of `slot`, or `None` on arithmetic overflow or an empty grid.
    #[must_use]
    pub fn feet(self, slot: u16) -> Option<[i32; 3]> {
        let columns = u32::from(self.columns);
        let slot = u32::from(slot);
        let column = i32::try_from(slot.checked_rem(columns)?).ok()?;
        let row = i32::try_from(slot / columns).ok()?;
        Some([
            self.origin[0].checked_add(column.checked_mul(self.spacing)?)?,
            0,
            self.origin[1].checked_add(row.checked_mul(self.spacing)?)?,
        ])
    }
}

impl Default for SpawnGrid {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZoneDefinition {
    revision: u64,
    gravity: [i32; 3],
    spawn_grid: SpawnGrid,
    colliders: Vec<StaticCollider>,
}

impl ZoneDefinition {
    /// Content with the [`SpawnGrid::DEFAULT`] spawn slots.
    pub fn new(
        revision: u64,
        gravity: [i32; 3],
        colliders: Vec<StaticCollider>,
    ) -> Result<Self, ZoneError> {
        Self::with_spawn_grid(revision, gravity, SpawnGrid::DEFAULT, colliders)
    }

    /// IDs occupy a disjoint namespace from player bodies. Sorting once gives
    /// stable construction, snapshots, and scene export independent of authoring order.
    /// Every collider bound and all `MAX_PLAYERS_PER_ZONE` spawn slots must lie
    /// within [`MAX_CONTENT_COORDINATE_UNITS`], and no spawned body may overlap a
    /// collider (touching, such as feet on the ground, is allowed).
    pub fn with_spawn_grid(
        revision: u64,
        gravity: [i32; 3],
        spawn_grid: SpawnGrid,
        mut colliders: Vec<StaticCollider>,
    ) -> Result<Self, ZoneError> {
        if colliders.len() > MAX_STATIC_COLLIDERS {
            return Err(ZoneError::new("zone static collider capacity reached"));
        }
        colliders.sort_by_key(|collider| collider.id);
        for collider in &colliders {
            if u64::from(collider.id) >= super::PLAYER_BODY_BASE {
                return Err(ZoneError::new(
                    "static collider id overlaps player body namespace",
                ));
            }
            if collider.half_extents.iter().any(|extent| *extent <= 0) {
                return Err(ZoneError::new("static collider extents must be positive"));
            }
            for (position, extent) in collider.position.iter().zip(collider.half_extents) {
                let (Some(max), Some(min)) =
                    (position.checked_add(extent), position.checked_sub(extent))
                else {
                    return Err(ZoneError::new("static collider bounds overflow"));
                };
                if !within_content_range(min) || !within_content_range(max) {
                    return Err(ZoneError::new(
                        "static collider lies outside the content coordinate range",
                    ));
                }
            }
        }
        if colliders.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(ZoneError::new("duplicate static collider id"));
        }
        validate_spawn_grid(spawn_grid, &colliders)?;
        Ok(Self {
            revision,
            gravity,
            spawn_grid,
            colliders,
        })
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn gravity(&self) -> [i32; 3] {
        self.gravity
    }

    #[must_use]
    pub const fn spawn_grid(&self) -> SpawnGrid {
        self.spawn_grid
    }

    #[must_use]
    pub fn colliders(&self) -> &[StaticCollider] {
        &self.colliders
    }
}

const fn within_content_range(value: i32) -> bool {
    -MAX_CONTENT_COORDINATE_UNITS <= value && value <= MAX_CONTENT_COORDINATE_UNITS
}

/// Spawned bodies may touch each other and colliders, but never overlap them.
fn validate_spawn_grid(grid: SpawnGrid, colliders: &[StaticCollider]) -> Result<(), ZoneError> {
    let body = PLAYER_HALF_EXTENTS_UNITS;
    if grid.columns == 0 || grid.spacing < 2 * body[0].max(body[2]) {
        return Err(ZoneError::new(
            "spawn grid needs columns and non-overlapping slots",
        ));
    }
    for slot in 0..MAX_PLAYERS_PER_ZONE {
        let feet = u16::try_from(slot)
            .ok()
            .and_then(|slot| grid.feet(slot))
            .ok_or_else(|| ZoneError::new("spawn grid overflows"))?;
        if !feet.iter().copied().all(within_content_range) {
            return Err(ZoneError::new(
                "spawn slot lies outside the content coordinate range",
            ));
        }
        let center = [feet[0], body[1], feet[2]];
        let blocked = colliders.iter().any(|collider| {
            (0..3).all(|axis| {
                let distance = (i64::from(center[axis]) - i64::from(collider.position[axis])).abs();
                distance < i64::from(body[axis]) + i64::from(collider.half_extents[axis])
            })
        });
        if blocked {
            return Err(ZoneError::new("spawn slot overlaps a static collider"));
        }
    }
    Ok(())
}

/// The first shared playable scene. Both hosts and native clients consume this
/// exact definition; changing geometry requires a new immutable revision.
#[must_use]
pub fn outpost_definition() -> ZoneDefinition {
    ZoneDefinition::new(
        1,
        [0, -1, 0],
        vec![
            StaticCollider {
                id: 1,
                position: [3_100, -50, 1_500],
                half_extents: [4_200, 50, 2_600],
            },
            StaticCollider {
                id: 2,
                position: [-650, 150, -500],
                half_extents: [180, 150, 170],
            },
            StaticCollider {
                id: 3,
                position: [650, 120, -500],
                half_extents: [160, 120, 190],
            },
            StaticCollider {
                id: 4,
                position: [300, 55, -250],
                half_extents: [55, 55, 55],
            },
            StaticCollider {
                id: 5,
                position: [-300, 135, -300],
                half_extents: [45, 135, 40],
            },
            StaticCollider {
                id: 6,
                position: [-1_100, 200, 1_500],
                half_extents: [30, 200, 2_600],
            },
            StaticCollider {
                id: 7,
                position: [7_300, 200, 1_500],
                half_extents: [30, 200, 2_600],
            },
            StaticCollider {
                id: 8,
                position: [3_100, 200, -1_100],
                half_extents: [4_200, 200, 30],
            },
            StaticCollider {
                id: 9,
                position: [3_100, 200, 4_100],
                half_extents: [4_200, 200, 30],
            },
        ],
    )
    .expect("built-in outpost geometry is valid")
}
