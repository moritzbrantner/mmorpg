//! Immutable collision content, shared by simulation construction and scene export.
//! Render meshes/materials are presentation data; these boxes define physical truth.

use crate::ZoneError;

pub const MAX_STATIC_COLLIDERS: usize = 1_024;
/// One render metre corresponds to 100 integer simulation units; Y points up.
pub const UNITS_PER_METRE: i32 = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollider {
    pub id: u32,
    pub position: [i32; 3],
    pub half_extents: [i32; 3],
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZoneDefinition {
    revision: u64,
    gravity: [i32; 3],
    colliders: Vec<StaticCollider>,
}

impl ZoneDefinition {
    /// IDs occupy a disjoint namespace from player bodies. Sorting once gives
    /// stable construction, snapshots, and scene export independent of authoring order.
    pub fn new(
        revision: u64,
        gravity: [i32; 3],
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
                if position.checked_add(extent).is_none() || position.checked_sub(extent).is_none()
                {
                    return Err(ZoneError::new("static collider bounds overflow"));
                }
            }
        }
        if colliders.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(ZoneError::new("duplicate static collider id"));
        }
        Ok(Self {
            revision,
            gravity,
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
    pub fn colliders(&self) -> &[StaticCollider] {
        &self.colliders
    }
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
