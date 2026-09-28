#![forbid(unsafe_code)]

mod content;
mod interest;
pub mod trig;
pub use content::{
    MAX_STATIC_COLLIDERS, StaticCollider, UNITS_PER_METRE, ZoneDefinition, outpost_definition,
};
use interest::InterestIndex;
pub use interest::{InterestMaintenanceStats, InterestQueryStats, PlayerProjection};

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use physics_engine::{Aabb, BodyId, RigidBody, Vec3i, World, WorldConfig};
use trig::{YAW_EIGHTH_TURN, YAW_QUARTER_TURN};

pub type PlayerId = u32;

pub const TICK_HZ: u16 = 30;
pub const MAX_PLAYERS_PER_ZONE: usize = 512;
pub const SNAPSHOT_SCHEMA_VERSION: u16 = 3;
pub const INTEREST_RADIUS_UNITS: i32 = 2_000;

const PLAYER_BODY_BASE: u64 = 1_000_000;
/// Character collision box: 0.6 m × 1.8 m × 0.6 m, shared with clients.
pub const PLAYER_HALF_EXTENTS_UNITS: [i32; 3] = [30, 90, 30];
const PLAYER_HALF_EXTENTS: Vec3i = Vec3i::new(
    PLAYER_HALF_EXTENTS_UNITS[0],
    PLAYER_HALF_EXTENTS_UNITS[1],
    PLAYER_HALF_EXTENTS_UNITS[2],
);
/// Horizontal speed for forward, strafe and forward-diagonal intent (6.3 m/s).
pub const RUN_SPEED_UNITS_PER_TICK: i32 = 21;
/// Horizontal speed whenever intent has a backward component.
pub const BACKPEDAL_SPEED_UNITS_PER_TICK: i32 = 13;
/// Upward velocity a grounded jump sets before physics applies gravity.
pub const JUMP_VELOCITY_UNITS_PER_TICK: i32 = 16;
/// The grounded probe spans this many units directly below the feet.
const GROUND_PROBE_DEPTH_UNITS: i32 = 2;
const SPAWN_GRID_WIDTH: u32 = 32;
const SPAWN_SPACING: i32 = 200;

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
}

/// Kind of a visible unit. Only players exist so far; creatures and NPCs join
/// this closed set when the zone gains them.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntityKind {
    Player,
}

/// One unit in a player-visible projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntitySnapshot {
    pub kind: EntityKind,
    pub id: u32,
    pub position: [i32; 3],
    /// Presentation-only velocity, saturated to the `i16` range. Canonical
    /// state keeps the exact physics velocity.
    pub velocity: [i16; 3],
    pub facing: u16,
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
}

/// A projection addressed to `viewer_id`. Entities are ordered by `(kind, id)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneSnapshot {
    pub content_revision: u64,
    pub acknowledged_sequence: u32,
    pub viewer_id: PlayerId,
    pub schema_version: u16,
    pub zone_id: ZoneId,
    pub tick: u64,
    pub entities: Vec<EntitySnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalZoneSnapshot {
    pub definition: ZoneDefinition,
    pub schema_version: u16,
    pub zone_id: ZoneId,
    pub tick: u64,
    pub players: Vec<CanonicalPlayerSnapshot>,
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

/// One validated movement axis; the wire and canonical form is `-1`, `0` or `1`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Axis {
    Negative,
    #[default]
    Zero,
    Positive,
}

impl Axis {
    fn new(value: i8) -> Result<Self, ZoneError> {
        match value {
            -1 => Ok(Self::Negative),
            0 => Ok(Self::Zero),
            1 => Ok(Self::Positive),
            _ => Err(ZoneError::new(
                "movement components must be between -1 and 1",
            )),
        }
    }

    const fn get(self) -> i8 {
        match self {
            Self::Negative => -1,
            Self::Zero => 0,
            Self::Positive => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PlayerState {
    facing: u16,
    forward: Axis,
    strafe: Axis,
    jump_pending: bool,
    last_sequence: u32,
    spawn_slot: u16,
}

pub struct ZoneSimulation {
    zone_id: ZoneId,
    tick: u64,
    world: World,
    definition: ZoneDefinition,
    players: BTreeMap<PlayerId, PlayerState>,
    interest: InterestIndex,
    interest_work: InterestMaintenanceStats,
}

impl ZoneSimulation {
    #[must_use]
    pub fn new(zone_id: ZoneId) -> Self {
        Self::with_definition(zone_id, ZoneDefinition::default())
            .expect("empty zone definition has no invalid physics bodies")
    }

    pub fn with_definition(zone_id: ZoneId, definition: ZoneDefinition) -> Result<Self, ZoneError> {
        let gravity = definition.gravity();
        let mut world = World::new(WorldConfig {
            gravity: Vec3i::new(gravity[0], gravity[1], gravity[2]),
            ..WorldConfig::default()
        });
        for collider in definition.colliders() {
            world
                .add_body(RigidBody::fixed(
                    BodyId(u64::from(collider.id)),
                    Vec3i::new(
                        collider.position[0],
                        collider.position[1],
                        collider.position[2],
                    ),
                    Vec3i::new(
                        collider.half_extents[0],
                        collider.half_extents[1],
                        collider.half_extents[2],
                    ),
                ))
                .map_err(physics_error)?;
        }
        Ok(Self {
            zone_id,
            tick: 0,
            world,
            definition,
            players: BTreeMap::new(),
            interest: InterestIndex::default(),
            interest_work: InterestMaintenanceStats::default(),
        })
    }

    #[must_use]
    pub fn definition(&self) -> &ZoneDefinition {
        &self.definition
    }

    pub fn from_snapshot(snapshot: CanonicalZoneSnapshot) -> Result<Self, ZoneError> {
        if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(ZoneError::new("unsupported snapshot schema version"));
        }
        if snapshot.players.len() > MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("zone player capacity reached"));
        }

        let mut zone = Self::with_definition(snapshot.zone_id, snapshot.definition)?;
        zone.tick = snapshot.tick;
        for player in snapshot.players {
            let forward = Axis::new(player.forward)?;
            let strafe = Axis::new(player.strafe)?;
            if usize::from(player.spawn_slot) >= MAX_PLAYERS_PER_ZONE {
                return Err(ZoneError::new("player spawn slot is out of range"));
            }
            if zone.players.contains_key(&player.player_id) {
                return Err(ZoneError::new("snapshot contains duplicate player"));
            }
            if zone
                .players
                .values()
                .any(|state| state.spawn_slot == player.spawn_slot)
            {
                return Err(ZoneError::new("snapshot contains duplicate spawn slot"));
            }

            zone.world
                .add_body(RigidBody::dynamic(
                    Self::body_id(player.player_id),
                    Vec3i::new(player.position[0], player.position[1], player.position[2]),
                    Vec3i::new(player.velocity[0], player.velocity[1], player.velocity[2]),
                    PLAYER_HALF_EXTENTS,
                ))
                .map_err(physics_error)?;
            zone.players.insert(
                player.player_id,
                PlayerState {
                    facing: player.facing,
                    forward,
                    strafe,
                    jump_pending: player.jump_pending,
                    last_sequence: player.last_sequence,
                    spawn_slot: player.spawn_slot,
                },
            );
        }
        zone.rebuild_interest()?;
        Ok(zone)
    }

    #[must_use]
    pub const fn zone_id(&self) -> ZoneId {
        self.zone_id
    }

    #[must_use]
    pub const fn current_tick(&self) -> u64 {
        self.tick
    }

    #[must_use]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    pub fn add_player(&mut self, player_id: PlayerId) -> Result<(), ZoneError> {
        if self.players.contains_key(&player_id) {
            return Err(ZoneError::new("player already exists in zone"));
        }
        if self.players.len() >= MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("zone player capacity reached"));
        }

        let spawn_slot = self
            .available_spawn_slot()
            .ok_or_else(|| ZoneError::new("zone spawn capacity reached"))?;

        self.world
            .add_body(RigidBody::dynamic(
                Self::body_id(player_id),
                Self::spawn_position(spawn_slot),
                Vec3i::ZERO,
                PLAYER_HALF_EXTENTS,
            ))
            .map_err(physics_error)?;

        let position = Self::spawn_position(spawn_slot);
        self.interest.insert(player_id, position.x, position.z);
        self.interest_work.bucket_inserts += 1;
        self.players.insert(
            player_id,
            PlayerState {
                spawn_slot,
                ..PlayerState::default()
            },
        );
        Ok(())
    }

    pub fn remove_player(&mut self, player_id: PlayerId) -> bool {
        if self.players.remove(&player_id).is_none() {
            return false;
        }
        self.world.remove_body(Self::body_id(player_id));
        self.interest.remove(player_id);
        self.interest_work.bucket_removes += 1;
        true
    }

    pub fn apply_command(
        &mut self,
        player_id: PlayerId,
        sequence: u32,
        command: ZoneCommand,
    ) -> Result<(), ZoneError> {
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        if sequence == 0 || sequence <= player.last_sequence {
            return Err(ZoneError::new("command sequence is stale"));
        }

        // Validate completely before mutating: a malformed command changes nothing.
        match command {
            ZoneCommand::Move {
                forward,
                strafe,
                facing,
            } => {
                let forward = Axis::new(forward)?;
                let strafe = Axis::new(strafe)?;
                player.forward = forward;
                player.strafe = strafe;
                player.facing = facing;
            }
            // Resolved during the next tick against the pre-step world.
            ZoneCommand::Jump => player.jump_pending = true,
        }
        player.last_sequence = sequence;
        Ok(())
    }

    pub fn advance_tick(&mut self) -> Result<(), ZoneError> {
        let next_tick = self
            .tick
            .checked_add(1)
            .ok_or_else(|| ZoneError::new("zone tick overflow"))?;
        for (&player_id, state) in &self.players {
            let body_id = Self::body_id(player_id);
            let body = self
                .world
                .body(body_id)
                .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
            // Setting velocities never moves bodies, so every grounded probe in
            // this pass observes the same pre-step positions regardless of order.
            let vertical_velocity = if state.jump_pending && is_grounded(&self.world, body)? {
                JUMP_VELOCITY_UNITS_PER_TICK
            } else {
                body.velocity().y
            };
            let mut velocity = movement_velocity(*state);
            velocity.y = vertical_velocity;
            self.world
                .set_velocity(body_id, velocity)
                .map_err(physics_error)?;
        }

        self.world.step(1).map_err(physics_error)?;
        // A jump intent is consumed by the tick that evaluated it, grounded or not.
        for state in self.players.values_mut() {
            state.jump_pending = false;
        }
        self.update_interest()?;
        self.tick = next_tick;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<CanonicalZoneSnapshot, ZoneError> {
        let players = self
            .players
            .iter()
            .map(|(&player_id, &state)| self.canonical_player_snapshot(player_id, state))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CanonicalZoneSnapshot {
            definition: self.definition.clone(),
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: self.zone_id,
            tick: self.tick,
            players,
        })
    }

    pub fn snapshot_for_player(&self, player_id: PlayerId) -> Result<ZoneSnapshot, ZoneError> {
        Ok(self.project_for_player(player_id)?.snapshot)
    }

    /// Cumulative work since creation or recovery; subtract readings around a workload.
    #[must_use]
    pub const fn interest_maintenance_stats(&self) -> InterestMaintenanceStats {
        self.interest_work
    }

    /// Queries the same authoritative projection as `snapshot_for_player`, with
    /// operation counts for workload analysis. Does not mutate canonical state.
    pub fn project_for_player(&self, player_id: PlayerId) -> Result<PlayerProjection, ZoneError> {
        let viewer = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        let center = self
            .world
            .body(Self::body_id(player_id))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?
            .position();

        let radius = i128::from(INTEREST_RADIUS_UNITS);
        let radius_squared = radius * radius;
        let mut entities = Vec::new();
        // Candidates arrive in ascending player ID order, which is `(kind, id)`
        // order while players are the only visible kind.
        let (candidates, stats) = self.interest.candidates(center.x, center.z);
        for candidate in candidates {
            let entity = self.player_entity(candidate)?;
            let dx = i128::from(entity.position[0]) - i128::from(center.x);
            let dz = i128::from(entity.position[2]) - i128::from(center.z);
            if (dx * dx) + (dz * dz) <= radius_squared {
                entities.push(entity);
            }
        }

        Ok(PlayerProjection {
            snapshot: ZoneSnapshot {
                content_revision: self.definition.revision(),
                acknowledged_sequence: viewer.last_sequence,
                viewer_id: player_id,
                schema_version: SNAPSHOT_SCHEMA_VERSION,
                zone_id: self.zone_id,
                tick: self.tick,
                entities,
            },
            stats,
        })
    }

    // Derived state is reconstructed on recovery, never serialized.
    fn rebuild_interest(&mut self) -> Result<(), ZoneError> {
        let mut interest = InterestIndex::default();
        for player_id in self.players.keys().copied() {
            let position = self
                .world
                .body(Self::body_id(player_id))
                .ok_or_else(|| ZoneError::new("player physics body is missing"))?
                .position();
            interest.insert(player_id, position.x, position.z);
        }
        self.interest = interest;
        self.interest_work.full_rebuilds += 1;
        self.interest_work.bucket_inserts += self.players.len();
        self.interest_work.players_inspected += self.players.len();
        Ok(())
    }

    fn update_interest(&mut self) -> Result<(), ZoneError> {
        // The pre-step velocity pass has already checked every player body.
        for &player_id in self.players.keys() {
            let position = self
                .world
                .body(Self::body_id(player_id))
                .ok_or_else(|| ZoneError::new("player physics body is missing"))?
                .position();
            self.interest_work.players_inspected += 1;
            if self
                .interest
                .move_if_needed(player_id, position.x, position.z)
            {
                self.interest_work.bucket_moves += 1;
                self.interest_work.bucket_removes += 1;
                self.interest_work.bucket_inserts += 1;
            }
        }
        Ok(())
    }

    fn player_entity(&self, player_id: PlayerId) -> Result<EntitySnapshot, ZoneError> {
        let state = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        let body = self
            .world
            .body(Self::body_id(player_id))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        let position = body.position();
        let velocity = body.velocity();
        Ok(EntitySnapshot {
            kind: EntityKind::Player,
            id: player_id,
            position: [position.x, position.y, position.z],
            velocity: [velocity.x, velocity.y, velocity.z].map(saturate_i16),
            facing: state.facing,
        })
    }

    fn canonical_player_snapshot(
        &self,
        player_id: PlayerId,
        state: PlayerState,
    ) -> Result<CanonicalPlayerSnapshot, ZoneError> {
        let body = self
            .world
            .body(Self::body_id(player_id))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        let position = body.position();
        let velocity = body.velocity();
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
        })
    }

    fn body_id(player_id: PlayerId) -> BodyId {
        BodyId(PLAYER_BODY_BASE + u64::from(player_id))
    }

    fn available_spawn_slot(&self) -> Option<u16> {
        (0..MAX_PLAYERS_PER_ZONE)
            .find(|candidate| {
                self.players
                    .values()
                    .all(|state| usize::from(state.spawn_slot) != *candidate)
            })
            .and_then(|slot| u16::try_from(slot).ok())
    }

    fn spawn_position(spawn_slot: u16) -> Vec3i {
        let spawn_slot = u32::from(spawn_slot);
        let column = i32::try_from(spawn_slot % SPAWN_GRID_WIDTH)
            .expect("spawn grid column always fits in i32");
        let row = i32::try_from(spawn_slot / SPAWN_GRID_WIDTH)
            .expect("spawn grid row always fits in i32");
        // Feet rest on y = 0, the top of flat walkable ground.
        Vec3i::new(
            column * SPAWN_SPACING,
            PLAYER_HALF_EXTENTS_UNITS[1],
            row * SPAWN_SPACING,
        )
    }
}

/// Horizontal controller velocity: `speed × direction(facing + local offset)`,
/// where the offset is one of eight multiples of 45°. The character's right is
/// `direction(facing - 90°)`. Zero intent yields zero horizontal velocity.
fn movement_velocity(state: PlayerState) -> Vec3i {
    const FORWARD: u16 = 0;
    const LEFT: u16 = YAW_QUARTER_TURN;
    const BACKWARD: u16 = 2 * YAW_QUARTER_TURN;
    const RIGHT: u16 = 3 * YAW_QUARTER_TURN;
    let offset = match (state.forward, state.strafe) {
        (Axis::Zero, Axis::Zero) => return Vec3i::ZERO,
        (Axis::Positive, Axis::Zero) => FORWARD,
        (Axis::Positive, Axis::Negative) => FORWARD + YAW_EIGHTH_TURN,
        (Axis::Zero, Axis::Negative) => LEFT,
        (Axis::Negative, Axis::Negative) => LEFT + YAW_EIGHTH_TURN,
        (Axis::Negative, Axis::Zero) => BACKWARD,
        (Axis::Negative, Axis::Positive) => BACKWARD + YAW_EIGHTH_TURN,
        (Axis::Zero, Axis::Positive) => RIGHT,
        (Axis::Positive, Axis::Positive) => RIGHT + YAW_EIGHTH_TURN,
    };
    let speed = match state.forward {
        Axis::Negative => BACKPEDAL_SPEED_UNITS_PER_TICK,
        Axis::Zero | Axis::Positive => RUN_SPEED_UNITS_PER_TICK,
    };
    let (x, z) = trig::direction(state.facing.wrapping_add(offset));
    Vec3i::new(trig::scale(speed, x), 0, trig::scale(speed, z))
}

/// A thin probe directly under the feet, inset by one unit horizontally so
/// touching a wall is not ground. The physics query counts touching bodies;
/// any body other than the player itself supports a jump.
fn is_grounded(world: &World, body: &RigidBody) -> Result<bool, ZoneError> {
    let position = body.position();
    let half_extents = body.half_extents();
    let probe_half_height = GROUND_PROBE_DEPTH_UNITS / 2;
    // Beyond the representable range there is no ground: the jump is ignored.
    let Some(probe_y) = position
        .y
        .checked_sub(half_extents.y)
        .and_then(|feet| feet.checked_sub(probe_half_height))
    else {
        return Ok(false);
    };
    let probe = Aabb::new(
        Vec3i::new(position.x, probe_y, position.z),
        Vec3i::new(
            (half_extents.x - 1).max(0),
            probe_half_height,
            (half_extents.z - 1).max(0),
        ),
    );
    let hits = world
        .overlap_query(probe)
        .map_err(|error| ZoneError::new(error.to_string()))?;
    Ok(hits.into_iter().any(|hit| hit != body.id()))
}

fn saturate_i16(value: i32) -> i16 {
    i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX })
}

fn physics_error(error: physics_engine::PhysicsError) -> ZoneError {
    ZoneError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_is_driven_through_the_shared_physics_engine() {
        let mut zone = ZoneSimulation::new(ZoneId::new(7));
        zone.add_player(1).unwrap();
        let before = zone.snapshot().unwrap().players[0].position;

        zone.apply_command(
            1,
            1,
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: YAW_QUARTER_TURN,
            },
        )
        .unwrap();
        for _ in 0..10 {
            zone.advance_tick().unwrap();
        }

        let after = zone.snapshot().unwrap().players[0].position;
        assert!(after[0] > before[0]);
        assert_eq!(after[2], before[2]);
    }

    #[test]
    fn configured_capacity_has_unique_spawn_positions() {
        use std::collections::BTreeSet;

        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        for index in 0..MAX_PLAYERS_PER_ZONE {
            let player_id = u32::try_from(index)
                .expect("configured capacity fits player id")
                .checked_mul(1_024)
                .and_then(|value| value.checked_add(1))
                .expect("test player id fits u32");
            zone.add_player(player_id).unwrap();
        }

        let positions = zone
            .snapshot()
            .unwrap()
            .players
            .into_iter()
            .map(|player| (player.position[0], player.position[2]))
            .collect::<BTreeSet<_>>();

        assert_eq!(positions.len(), MAX_PLAYERS_PER_ZONE);
    }

    #[test]
    fn canonical_snapshot_restores_movement_sequence_and_spawn_allocation() {
        let mut original = ZoneSimulation::new(ZoneId::new(1));
        original.add_player(1).unwrap();
        original
            .apply_command(
                1,
                6,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: -1,
                    facing: 1_234,
                },
            )
            .unwrap();
        original.apply_command(1, 7, ZoneCommand::Jump).unwrap();

        let snapshot = original.snapshot().unwrap();
        assert_eq!(snapshot.players[0].facing, 1_234);
        assert_eq!(snapshot.players[0].forward, 1);
        assert_eq!(snapshot.players[0].strafe, -1);
        assert!(snapshot.players[0].jump_pending);
        assert_eq!(snapshot.players[0].last_sequence, 7);

        let mut recovered = ZoneSimulation::from_snapshot(snapshot).unwrap();
        assert_eq!(
            recovered
                .apply_command(1, 7, ZoneCommand::Jump)
                .unwrap_err()
                .message(),
            "command sequence is stale"
        );

        original.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), original.snapshot().unwrap());

        recovered.add_player(1_025).unwrap();
        let recovered = recovered.snapshot().unwrap();
        assert_ne!(
            recovered.players[0].spawn_slot,
            recovered.players[1].spawn_slot
        );
    }

    #[test]
    fn player_projection_fails_closed_when_physics_state_is_incomplete() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        zone.world.remove_body(ZoneSimulation::body_id(2));

        let error = zone.snapshot_for_player(1).unwrap_err();

        assert_eq!(error.message(), "player physics body is missing");
    }

    #[test]
    fn stale_commands_fail_closed() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        let run = ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 0,
        };
        zone.apply_command(1, 5, run).unwrap();

        for (sequence, command) in [
            (5, run),
            (4, ZoneCommand::Jump),
            (5, ZoneCommand::Jump),
            (0, ZoneCommand::Jump),
        ] {
            let error = zone.apply_command(1, sequence, command).unwrap_err();
            assert_eq!(error.message(), "command sequence is stale");
        }
        let player = &zone.snapshot().unwrap().players[0];
        assert!(!player.jump_pending, "rejected commands change nothing");
        assert_eq!(player.last_sequence, 5);
    }

    #[test]
    fn malformed_movement_fails_without_consuming_the_sequence() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        for (forward, strafe) in [(2, 0), (0, -2), (i8::MIN, 1), (1, i8::MAX)] {
            let error = zone
                .apply_command(
                    1,
                    3,
                    ZoneCommand::Move {
                        forward,
                        strafe,
                        facing: 9,
                    },
                )
                .unwrap_err();
            assert_eq!(
                error.message(),
                "movement components must be between -1 and 1"
            );
        }
        let player = &zone.snapshot().unwrap().players[0];
        assert_eq!(
            (
                player.facing,
                player.forward,
                player.strafe,
                player.last_sequence
            ),
            (0, 0, 0, 0)
        );
        zone.apply_command(
            1,
            3,
            ZoneCommand::Move {
                forward: -1,
                strafe: 1,
                facing: 9,
            },
        )
        .unwrap();
    }

    #[test]
    fn recovery_rejects_out_of_range_movement_intent() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        let valid = zone.snapshot().unwrap();
        for (forward, strafe) in [(2, 0), (0, -2)] {
            let mut invalid = valid.clone();
            invalid.players[0].forward = forward;
            invalid.players[0].strafe = strafe;
            assert_eq!(
                ZoneSimulation::from_snapshot(invalid)
                    .err()
                    .unwrap()
                    .message(),
                "movement components must be between -1 and 1"
            );
        }
        let mut invalid_slot = valid;
        invalid_slot.players[0].spawn_slot = u16::try_from(MAX_PLAYERS_PER_ZONE).unwrap();
        assert!(ZoneSimulation::from_snapshot(invalid_slot).is_err());
    }

    #[test]
    fn player_snapshot_applies_zone_owned_interest_policy() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        for player_id in 1..=12 {
            zone.add_player(player_id).unwrap();
        }

        let canonical = zone.snapshot().unwrap();
        let visible = zone.snapshot_for_player(1).unwrap();

        assert_eq!(canonical.players.len(), 12);
        assert_eq!(visible.viewer_id, 1);
        assert_eq!(visible.entities.len(), 11);
        assert_eq!(visible.entities[0].id, 1);
        assert!(
            visible
                .entities
                .iter()
                .all(|entity| entity.kind == EntityKind::Player && entity.id != 12)
        );
    }

    #[test]
    fn projection_carries_facing_and_saturates_presentation_velocity() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        zone.apply_command(
            2,
            1,
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: 40_000,
            },
        )
        .unwrap();
        let mut canonical = zone.snapshot().unwrap();
        canonical.players[0].velocity = [70_000, -70_000, 32_767];
        let zone = ZoneSimulation::from_snapshot(canonical).unwrap();

        let visible = zone.snapshot_for_player(2).unwrap();

        assert_eq!(visible.viewer_id, 2);
        assert_eq!(visible.acknowledged_sequence, 1);
        assert_eq!(visible.entities[0].velocity, [i16::MAX, i16::MIN, i16::MAX]);
        assert_eq!(visible.entities[0].facing, 0);
        assert_eq!(visible.entities[1].facing, 40_000);
        assert_eq!(
            zone.snapshot().unwrap().players[0].velocity,
            [70_000, -70_000, 32_767],
            "canonical state keeps exact velocity"
        );
    }
}
