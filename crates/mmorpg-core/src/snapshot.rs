//! Canonical snapshots: every authoritative field needed to continue a zone
//! exactly, for trusted replay and recovery. Content is referenced by
//! revision and fingerprint, never embedded: recovery needs the identical
//! [`ZoneContent`] and fails closed on any mismatch.

use std::collections::BTreeSet;
use std::sync::Arc;

use physics_engine::RigidBody;

use crate::ai::is_wander_point;
use crate::content::within_content_range;
use crate::creature::{CreatureState, Life, MAX_THREAT_ENTRIES};
use crate::entity::body_id;
use crate::rng::ZoneRng;
use crate::unit::{MAX_UNIT_LEVEL, PLAYER_START_LEVEL, player_max_health};
use crate::zone::{
    Axis, MAX_PENDING_INTENTS, PLAYER_HALF_EXTENTS, PlayerState, physics_error, vector,
};
use crate::{
    CreatureId, EntityRef, MAX_EVENTS_PER_PLAYER, MAX_PLAYERS_PER_ZONE, PlayerId,
    SNAPSHOT_SCHEMA_VERSION, ZoneContent, ZoneError, ZoneEvent, ZoneId, ZoneSimulation,
};

/// A queued discrete intent, consumed by the next tick in sequence order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerIntent {
    SelectTarget(Option<EntityRef>),
    StartAttack,
    StopAttack,
    ReleaseSpirit,
}

/// A creature's decision state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreatureAi {
    /// Waiting `timer` ticks, or walking to `destination` for at most `timer` ticks.
    Idle {
        timer: u16,
        destination: Option<[i32; 2]>,
    },
    /// Fighting its threat leader.
    Engaged,
    /// Running home for `ticks` ticks so far.
    Evading { ticks: u16 },
}

/// A creature's life cycle; the tick numbers are those of its death.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreatureLife {
    Alive,
    Corpse { died_at: u64 },
    Despawned { died_at: u64 },
}

/// One threat table row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreatEntry {
    pub entity: EntityRef,
    pub threat: u32,
}

/// Unit and combat state of a player.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalPlayerCombat {
    pub level: u8,
    /// Zero means dead.
    pub health: u32,
    pub target: Option<EntityRef>,
    pub auto_attack: bool,
    pub swing_timer: u16,
    pub combat_timer: u16,
    pub calm_ticks: u16,
    pub error_cooldown: u16,
    /// Queued intents in sequence order.
    pub intents: Vec<PlayerIntent>,
    /// A later intent found the queue full and was dropped; the next tick
    /// reports it. Only a full queue drops intents.
    pub intents_dropped: bool,
    /// Events of the current tick.
    pub events: Vec<ZoneEvent>,
}

impl Default for CanonicalPlayerCombat {
    /// A freshly admitted player: level 1, full health, no target.
    fn default() -> Self {
        Self {
            level: PLAYER_START_LEVEL,
            health: player_max_health(PLAYER_START_LEVEL),
            target: None,
            auto_attack: false,
            swing_timer: 0,
            combat_timer: 0,
            calm_ticks: 0,
            error_cooldown: 0,
            intents: Vec::new(),
            intents_dropped: false,
            events: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalPlayerSnapshot {
    pub player_id: PlayerId,
    pub position: [i32; 3],
    pub velocity: [i32; 3],
    pub facing: u16,
    pub forward: i8,
    pub strafe: i8,
    pub jump_pending: bool,
    pub last_sequence: u32,
    pub spawn_slot: u16,
    pub combat: CanonicalPlayerCombat,
}

/// A creature; `position` is its body or corpse, zero once despawned, and
/// `velocity` is zero unless it is alive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalCreatureSnapshot {
    pub creature_id: CreatureId,
    pub level: u8,
    pub health: u32,
    pub facing: u16,
    pub position: [i32; 3],
    pub velocity: [i32; 3],
    pub life: CreatureLife,
    pub ai: CreatureAi,
    /// Ordered by entity.
    pub threat: Vec<ThreatEntry>,
    pub swing_timer: u16,
    pub combat_timer: u16,
    pub tapped_by: Option<PlayerId>,
}

/// Everything authoritative about a zone at a tick boundary, with its
/// content referenced by identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalZoneSnapshot {
    pub schema_version: u16,
    pub zone_id: ZoneId,
    pub tick: u64,
    pub content_revision: u64,
    pub content_fingerprint: u64,
    pub rng_state: u64,
    /// Ordered by player ID.
    pub players: Vec<CanonicalPlayerSnapshot>,
    /// One record per content spawn, in creature-ID order.
    pub creatures: Vec<CanonicalCreatureSnapshot>,
}

impl ZoneSimulation {
    pub fn snapshot(&self) -> Result<CanonicalZoneSnapshot, ZoneError> {
        let players = self
            .players
            .iter()
            .map(|(&player_id, state)| {
                let body = self
                    .world
                    .body(body_id(EntityRef::Player(player_id)))
                    .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
                let (position, velocity) = (body.position(), body.velocity());
                Ok(CanonicalPlayerSnapshot {
                    player_id,
                    position: [position.x, position.y, position.z],
                    velocity: [velocity.x, velocity.y, velocity.z],
                    facing: state.facing,
                    forward: state.forward.get(),
                    strafe: state.strafe.get(),
                    jump_pending: state.jump_pending,
                    last_sequence: state.last_sequence,
                    spawn_slot: state.spawn_slot,
                    combat: CanonicalPlayerCombat {
                        level: state.level,
                        health: state.health,
                        target: state.target,
                        auto_attack: state.auto_attack,
                        swing_timer: state.swing_timer,
                        combat_timer: state.combat_timer,
                        calm_ticks: state.calm_ticks,
                        error_cooldown: state.error_cooldown,
                        intents: state.intents.clone(),
                        intents_dropped: state.intents_dropped,
                        events: state.events.clone(),
                    },
                })
            })
            .collect::<Result<Vec<_>, ZoneError>>()?;
        let creatures = self
            .creatures
            .iter()
            .map(|(&creature_id, creature)| {
                let (position, velocity, life) = match creature.life {
                    Life::Alive => {
                        let body = self
                            .world
                            .body(body_id(EntityRef::Creature(creature_id)))
                            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?;
                        let (position, velocity) = (body.position(), body.velocity());
                        (
                            [position.x, position.y, position.z],
                            [velocity.x, velocity.y, velocity.z],
                            CreatureLife::Alive,
                        )
                    }
                    Life::Corpse { died_at, position } => {
                        (position, [0; 3], CreatureLife::Corpse { died_at })
                    }
                    Life::Despawned { died_at } => {
                        ([0; 3], [0; 3], CreatureLife::Despawned { died_at })
                    }
                };
                Ok(CanonicalCreatureSnapshot {
                    creature_id,
                    level: creature.level,
                    health: creature.health,
                    facing: creature.facing,
                    position,
                    velocity,
                    life,
                    ai: creature.ai,
                    threat: creature.threat.clone(),
                    swing_timer: creature.swing_timer,
                    combat_timer: creature.combat_timer,
                    tapped_by: creature.tapped_by,
                })
            })
            .collect::<Result<Vec<_>, ZoneError>>()?;
        Ok(CanonicalZoneSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: self.zone_id,
            tick: self.tick,
            content_revision: self.content.revision(),
            content_fingerprint: self.content.fingerprint(),
            rng_state: self.rng.state(),
            players,
            creatures,
        })
    }

    /// Restores a zone from a canonical snapshot and the content it was
    /// taken with. Any content identity mismatch, schema mismatch or
    /// inconsistent state fails closed; the restored zone continues exactly.
    pub fn from_snapshot(
        snapshot: CanonicalZoneSnapshot,
        content: Arc<ZoneContent>,
    ) -> Result<Self, ZoneError> {
        if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(ZoneError::new("unsupported snapshot schema version"));
        }
        if snapshot.content_revision != content.revision()
            || snapshot.content_fingerprint != content.fingerprint()
        {
            return Err(ZoneError::new(
                "snapshot content identity does not match the supplied zone content",
            ));
        }
        if snapshot.players.len() > MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("zone player capacity reached"));
        }
        let mut zone = Self::without_creatures(snapshot.zone_id, content)?;
        zone.tick = snapshot.tick;
        zone.rng = ZoneRng::from_state(snapshot.rng_state);
        let mut spawn_slots = BTreeSet::new();
        for player in snapshot.players {
            zone.restore_player(player, &mut spawn_slots)?;
        }
        if snapshot.creatures.len() != zone.content.creature_spawns().len() {
            return Err(ZoneError::new(
                "snapshot creatures do not match the content spawns",
            ));
        }
        for record in snapshot.creatures {
            zone.restore_creature(record)?;
        }
        zone.validate_references()?;
        zone.rebuild_interest()?;
        Ok(zone)
    }

    fn restore_player(
        &mut self,
        player: CanonicalPlayerSnapshot,
        spawn_slots: &mut BTreeSet<u16>,
    ) -> Result<(), ZoneError> {
        let forward = Axis::new(player.forward)?;
        let strafe = Axis::new(player.strafe)?;
        if usize::from(player.spawn_slot) >= MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("player spawn slot is out of range"));
        }
        if self.players.contains_key(&player.player_id) {
            return Err(ZoneError::new("snapshot contains duplicate player"));
        }
        if !spawn_slots.insert(player.spawn_slot) {
            return Err(ZoneError::new("snapshot contains duplicate spawn slot"));
        }
        let combat = player.combat;
        if !(1..=MAX_UNIT_LEVEL).contains(&combat.level)
            || combat.health > player_max_health(combat.level)
        {
            return Err(ZoneError::new("player unit state is out of range"));
        }
        if combat.intents.len() > MAX_PENDING_INTENTS || combat.events.len() > MAX_EVENTS_PER_PLAYER
        {
            return Err(ZoneError::new("player queues exceed their capacity"));
        }
        if combat.intents_dropped && combat.intents.len() != MAX_PENDING_INTENTS {
            return Err(ZoneError::new("only a full intent queue drops intents"));
        }
        if combat.health == 0 && combat.auto_attack {
            return Err(ZoneError::new("a dead player cannot auto-attack"));
        }
        self.world
            .add_body(RigidBody::dynamic(
                body_id(EntityRef::Player(player.player_id)),
                vector(player.position),
                vector(player.velocity),
                PLAYER_HALF_EXTENTS,
            ))
            .map_err(physics_error)?;
        self.players.insert(
            player.player_id,
            PlayerState {
                facing: player.facing,
                forward,
                strafe,
                jump_pending: player.jump_pending,
                last_sequence: player.last_sequence,
                spawn_slot: player.spawn_slot,
                level: combat.level,
                health: combat.health,
                target: combat.target,
                auto_attack: combat.auto_attack,
                swing_timer: combat.swing_timer,
                combat_timer: combat.combat_timer,
                calm_ticks: combat.calm_ticks,
                error_cooldown: combat.error_cooldown,
                intents: combat.intents,
                intents_dropped: combat.intents_dropped,
                events: combat.events,
            },
        );
        Ok(())
    }

    fn restore_creature(&mut self, record: CanonicalCreatureSnapshot) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let expected = content
            .creature_spawns()
            .get(self.creatures.len())
            .map(|spawn| spawn.id);
        if expected != Some(record.creature_id) {
            return Err(ZoneError::new(
                "snapshot creatures do not match the content spawns",
            ));
        }
        let (spawn, template) = Self::creature_content(&content, record.creature_id)?;
        // Bodies and corpses stay inside the world limits and wander points
        // near their spawn, so AI arithmetic stays in range.
        let wanders_off = matches!(
            record.ai,
            CreatureAi::Idle { destination: Some(to), .. }
                if !is_wander_point(spawn.position, to, spawn.wander_radius)
        );
        if !record.position.into_iter().all(within_content_range) || wanders_off {
            return Err(ZoneError::new(
                "creature position or wander destination is out of range",
            ));
        }
        if !(template.min_level..=template.max_level).contains(&record.level)
            || record.health > template.max_health(record.level)
            || record.threat.len() > MAX_THREAT_ENTRIES
            || record
                .threat
                .windows(2)
                .any(|pair| pair[0].entity >= pair[1].entity)
        {
            return Err(ZoneError::new("creature state is out of range"));
        }
        let at_rest = record.ai
            == CreatureAi::Idle {
                timer: 0,
                destination: None,
            }
            && record.threat.is_empty()
            && record.swing_timer == 0
            && record.combat_timer == 0
            && record.velocity == [0; 3]
            && record.health == 0;
        let life = match record.life {
            CreatureLife::Alive => {
                if record.health == 0 {
                    return Err(ZoneError::new("a living creature needs health"));
                }
                self.world
                    .add_body(RigidBody::dynamic(
                        body_id(EntityRef::Creature(record.creature_id)),
                        vector(record.position),
                        vector(record.velocity),
                        vector(template.half_extents),
                    ))
                    .map_err(physics_error)?;
                Life::Alive
            }
            CreatureLife::Corpse { died_at } if at_rest && died_at <= self.tick => Life::Corpse {
                died_at,
                position: record.position,
            },
            CreatureLife::Despawned { died_at }
                if at_rest && died_at <= self.tick && record.position == [0; 3] =>
            {
                Life::Despawned { died_at }
            }
            CreatureLife::Corpse { .. } | CreatureLife::Despawned { .. } => {
                return Err(ZoneError::new("dead creature state is inconsistent"));
            }
        };
        if !matches!(record.ai, CreatureAi::Engaged) && !record.threat.is_empty() {
            return Err(ZoneError::new("only engaged creatures have threat"));
        }
        self.creatures.insert(
            record.creature_id,
            CreatureState {
                level: record.level,
                health: record.health,
                facing: record.facing,
                life,
                ai: record.ai,
                threat: record.threat,
                swing_timer: record.swing_timer,
                combat_timer: record.combat_timer,
                tapped_by: record.tapped_by,
            },
        );
        Ok(())
    }

    /// Player selections and threat tables name units that exist; threat
    /// tables hold only living players.
    fn validate_references(&self) -> Result<(), ZoneError> {
        let exists = |entity: EntityRef| match entity {
            EntityRef::Player(player_id) => self.players.contains_key(&player_id),
            EntityRef::Creature(creature_id) => self.creatures.contains_key(&creature_id),
            EntityRef::Npc(npc_id) => self.content.npc(npc_id).is_some(),
        };
        for player in self.players.values() {
            if player.target.is_some_and(|target| !exists(target)) {
                return Err(ZoneError::new("player target does not exist"));
            }
        }
        for creature in self.creatures.values() {
            for entry in &creature.threat {
                let EntityRef::Player(player_id) = entry.entity else {
                    return Err(ZoneError::new("only players can be on a threat table"));
                };
                if !self
                    .players
                    .get(&player_id)
                    .is_some_and(PlayerState::is_alive)
                {
                    return Err(ZoneError::new("threat table names an absent player"));
                }
            }
        }
        Ok(())
    }
}
