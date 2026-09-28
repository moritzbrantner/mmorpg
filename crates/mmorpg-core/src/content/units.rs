//! Unit tables of zone content: creature templates, creature spawns and NPCs.
//! They are plain data; [`super::ZoneContent::new`] validates them once.

use crate::{CreatureId, CreatureTemplateId, NpcId};

/// Longest template or NPC name in bytes.
pub const MAX_UNIT_NAME_BYTES: usize = 64;
/// Upper bounds per content revision.
pub const MAX_CREATURE_TEMPLATES: usize = 256;
pub const MAX_CREATURE_SPAWNS: usize = 1_024;
pub const MAX_NPCS: usize = 256;
/// Largest wander radius around a spawn point (20 m).
pub const MAX_WANDER_RADIUS_UNITS: i32 = 2_000;
/// Largest creature half-extent on any axis (5 m).
pub const MAX_CREATURE_HALF_EXTENT_UNITS: i32 = 500;

/// Creatures of one family assist each other.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CreatureFamily {
    Wolf,
    Boar,
    Vermin,
    Marauder,
    Mirefin,
    Redbrand,
}

impl CreatureFamily {
    /// Every family, in order.
    pub const ALL: [Self; 6] = [
        Self::Wolf,
        Self::Boar,
        Self::Vermin,
        Self::Marauder,
        Self::Mirefin,
        Self::Redbrand,
    ];

    /// Stable lower-case name for exports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Wolf => "wolf",
            Self::Boar => "boar",
            Self::Vermin => "vermin",
            Self::Marauder => "marauder",
            Self::Mirefin => "mirefin",
            Self::Redbrand => "redbrand",
        }
    }
}

/// Whether a creature starts fights.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CreatureBehaviour {
    /// Engages the nearest living player within its aggro radius.
    Aggressive,
    /// Engages only when attacked.
    Neutral,
}

impl CreatureBehaviour {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Aggressive => "aggressive",
            Self::Neutral => "neutral",
        }
    }
}

/// Numbers and shape shared by every creature of one kind. Health and damage
/// are given at `min_level`; each level above it adds the per-level amounts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatureTemplate {
    pub id: CreatureTemplateId,
    pub name: String,
    pub family: CreatureFamily,
    pub behaviour: CreatureBehaviour,
    pub min_level: u8,
    pub max_level: u8,
    pub elite: bool,
    pub health: u32,
    pub health_per_level: u32,
    /// Inclusive melee damage range at `min_level`.
    pub damage: [u16; 2],
    pub damage_per_level: u16,
    pub swing_ticks: u16,
    /// Ticks from death to respawn; at least the corpse duration.
    pub respawn_ticks: u32,
    /// Collision box; its feet rest on y = 0 at the spawn point.
    pub half_extents: [i32; 3],
}

impl CreatureTemplate {
    /// Maximum health at `level`, clamped into the template's level range.
    /// Validation proves the maximum level cannot overflow.
    #[must_use]
    pub const fn max_health(&self, level: u8) -> u32 {
        let above = self.levels_above_min(level);
        self.health + self.health_per_level * above as u32
    }

    /// Inclusive melee damage range at `level`, clamped like [`Self::max_health`].
    #[must_use]
    pub const fn damage_at(&self, level: u8) -> [u16; 2] {
        let bonus = self.damage_per_level * self.levels_above_min(level) as u16;
        [self.damage[0] + bonus, self.damage[1] + bonus]
    }

    const fn levels_above_min(&self, level: u8) -> u8 {
        let level = if level > self.max_level {
            self.max_level
        } else {
            level
        };
        level.saturating_sub(self.min_level)
    }
}

/// One creature placement. The creature it hosts has the same ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatureSpawn {
    pub id: CreatureId,
    pub template: CreatureTemplateId,
    /// Feet position on y = 0, XZ in units.
    pub position: [i32; 2],
    pub facing: u16,
    /// Idle creatures wander within this XZ distance of `position`.
    pub wander_radius: i32,
}

/// What an NPC does for players; later steps give each role behaviour.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NpcRole {
    QuestGiver,
    Vendor,
    SpiritHealer,
    Guard,
}

impl NpcRole {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::QuestGiver => "quest_giver",
            Self::Vendor => "vendor",
            Self::SpiritHealer => "spirit_healer",
            Self::Guard => "guard",
        }
    }
}

/// A friendly, unattackable character standing at a post. NPCs have
/// character-sized fixed bodies and no AI yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Npc {
    pub id: NpcId,
    pub name: String,
    pub role: NpcRole,
    pub level: u8,
    /// Feet position on y = 0, XZ in units.
    pub position: [i32; 2],
    pub facing: u16,
}
