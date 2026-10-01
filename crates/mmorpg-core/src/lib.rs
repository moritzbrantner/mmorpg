#![forbid(unsafe_code)]

//! Deterministic zone-local gameplay: movement, units, combat, creature AI
//! and the interest/visibility policy of player projections.
//!
//! [`ZoneSimulation`] advances in the fixed tick order documented on
//! [`ZoneSimulation::advance_tick`]. Physics integration and collision stay in
//! `physics-engine`; wire formats live in `mmorpg-protocol`.

mod ai;
mod areas;
mod combat;
mod content;
mod creature;
mod entity;
mod events;
mod interest;
mod inventory;
mod progression;
mod projection;
pub mod rng;
mod snapshot;
pub mod trig;
pub mod unit;
mod zone;

pub use areas::{Area, AreaId, MAX_AREA_NAME_BYTES, MAX_ZONE_AREAS, ZoneAreas};
pub use content::greyhaven_vale::{self, greyhaven_vale_definition};
pub use content::{
    CreatureBehaviour, CreatureFamily, CreatureSpawn, CreatureTemplate,
    MAX_CONTENT_COORDINATE_UNITS, MAX_CREATURE_HALF_EXTENT_UNITS, MAX_CREATURE_SPAWNS,
    MAX_CREATURE_TEMPLATES, MAX_NPCS, MAX_STATIC_COLLIDERS, MAX_UNIT_NAME_BYTES,
    MAX_WANDER_RADIUS_UNITS, Npc, NpcRole, SpawnGrid, StaticCollider, UNITS_PER_METRE, XzBounds,
    ZoneContent, ZoneDefinition,
};
pub use creature::{
    AGGRO_MAX_RADIUS_UNITS, AGGRO_MIN_RADIUS_UNITS, AGGRO_PER_LEVEL_UNITS, AGGRO_RADIUS_UNITS,
    ARRIVAL_RADIUS_UNITS, ASSIST_RADIUS_UNITS, CREATURE_EVADE_SPEED_UNITS_PER_TICK,
    CREATURE_RUN_SPEED_UNITS_PER_TICK, CREATURE_WALK_SPEED_UNITS_PER_TICK, EVADE_TIMEOUT_TICKS,
    LEASH_RADIUS_UNITS, MAX_THREAT_ENTRIES, WANDER_WAIT_TICKS, WANDER_WALK_TIMEOUT_TICKS,
};
pub use entity::{CreatureId, CreatureTemplateId, EntityKind, EntityRef, NpcId};
pub use events::{ErrorCode, MAX_EVENTS_PER_PLAYER, ZoneEvent};
pub use interest::{InterestMaintenanceStats, InterestQueryStats, PlayerProjection};
pub use inventory::{
    INVENTORY_SLOTS, ITEM_CATALOG, ITEM_CATALOG_REVISION, Inventory, InventoryError, ItemId,
    ItemStack, ItemTemplate, item_template,
};
pub use progression::{MAX_PLAYER_LEVEL, experience_to_next_level, kill_experience};
pub use projection::{EntityFlags, EntitySnapshot, ViewerState, ZoneSnapshot};
pub use snapshot::{
    CanonicalCreatureSnapshot, CanonicalPlayerCombat, CanonicalPlayerSnapshot,
    CanonicalZoneSnapshot, CreatureAi, CreatureLife, PlayerIntent, ThreatEntry,
};
pub use zone::{MAX_PENDING_INTENTS, ZoneSimulation, ZoneTickWork};

use std::error::Error;
use std::fmt;

pub type PlayerId = u32;

pub const TICK_HZ: u16 = 30;
pub const MAX_PLAYERS_PER_ZONE: usize = 512;
/// Core schema of canonical and player-visible snapshots.
pub const SNAPSHOT_SCHEMA_VERSION: u16 = 6;
/// Inclusive XZ radius of player-scoped relevance (45 m).
pub const INTEREST_RADIUS_UNITS: i32 = 4_500;
/// Deterministic relevance cap of one player projection: the viewer, its
/// target and the nearest units. `mmorpg-protocol` packs a projection into
/// its per-datagram byte budget in this priority order.
pub const MAX_VISIBLE_ENTITIES: usize = 64;

/// Character collision box: 0.6 m × 1.8 m × 0.6 m, shared with clients.
pub const PLAYER_HALF_EXTENTS_UNITS: [i32; 3] = [30, 90, 30];
/// Horizontal speed for forward, strafe and forward-diagonal intent (6.3 m/s).
pub const RUN_SPEED_UNITS_PER_TICK: i32 = 21;
/// Horizontal speed whenever intent has a backward component.
pub const BACKPEDAL_SPEED_UNITS_PER_TICK: i32 = 13;
/// Upward velocity a grounded jump sets before physics applies gravity.
pub const JUMP_VELOCITY_UNITS_PER_TICK: i32 = 16;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ZoneId(u32);

impl ZoneId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Player intent. The session runtime supplies identity and sequence separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneCommand {
    /// Held movement relative to `facing`: `forward` and `strafe` are each in
    /// `-1..=1`; positive strafe is the character's right.
    Move {
        forward: i8,
        strafe: i8,
        facing: u16,
    },
    /// Edge-triggered jump, honoured on the next tick only when grounded.
    Jump,
    /// Selects a unit, or clears the selection with `None`.
    SelectTarget(Option<EntityRef>),
    /// Starts auto-attacking the selected target.
    StartAttack,
    StopAttack,
    /// Returns a dead player to the graveyard with half health.
    ReleaseSpirit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneError {
    message: String,
}

impl ZoneError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ZoneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ZoneError {}
