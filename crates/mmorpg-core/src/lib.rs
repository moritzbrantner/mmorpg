#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use physics_engine::{BodyId, RigidBody, Vec3i, World, WorldConfig};

pub type PlayerId = u32;

pub const TICK_HZ: u16 = 30;
pub const MAX_PLAYERS_PER_ZONE: usize = 512;
pub const SNAPSHOT_SCHEMA_VERSION: u16 = 1;
pub const INTEREST_RADIUS_UNITS: i32 = 2_000;

const PLAYER_BODY_BASE: u64 = 1_000_000;
const PLAYER_HALF_EXTENTS: Vec3i = Vec3i::new(30, 50, 30);
const PLAYER_SPEED: i32 = 12;
const PLAYER_DIAGONAL_SPEED: i32 = 8;
const SPAWN_GRID_WIDTH: u32 = 16;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneCommand {
    SetMovement { x: i8, z: i8 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerSnapshot {
    pub player_id: PlayerId,
    pub position: [i32; 3],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneSnapshot {
    pub schema_version: u16,
    pub zone_id: ZoneId,
    pub tick: u64,
    pub players: Vec<PlayerSnapshot>,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PlayerState {
    movement_x: i8,
    movement_z: i8,
    last_sequence: u32,
}

pub struct ZoneSimulation {
    zone_id: ZoneId,
    tick: u64,
    world: World,
    players: BTreeMap<PlayerId, PlayerState>,
}

impl ZoneSimulation {
    #[must_use]
    pub fn new(zone_id: ZoneId) -> Self {
        Self {
            zone_id,
            tick: 0,
            world: World::new(WorldConfig {
                gravity: Vec3i::ZERO,
                ..WorldConfig::default()
            }),
            players: BTreeMap::new(),
        }
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

        self.world
            .add_body(RigidBody::dynamic(
                Self::body_id(player_id),
                Self::spawn_position(player_id),
                Vec3i::ZERO,
                PLAYER_HALF_EXTENTS,
            ))
            .map_err(physics_error)?;

        self.players.insert(player_id, PlayerState::default());
        Ok(())
    }

    pub fn remove_player(&mut self, player_id: PlayerId) -> bool {
        if self.players.remove(&player_id).is_none() {
            return false;
        }
        self.world.remove_body(Self::body_id(player_id));
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

        match command {
            ZoneCommand::SetMovement { x, z } => {
                if !(-1..=1).contains(&x) || !(-1..=1).contains(&z) {
                    return Err(ZoneError::new(
                        "movement components must be between -1 and 1",
                    ));
                }
                player.movement_x = x;
                player.movement_z = z;
            }
        }
        player.last_sequence = sequence;
        Ok(())
    }

    pub fn advance_tick(&mut self) -> Result<(), ZoneError> {
        for (&player_id, state) in &self.players {
            self.world
                .set_velocity(Self::body_id(player_id), Self::movement_velocity(*state))
                .map_err(physics_error)?;
        }

        self.world.step(1).map_err(physics_error)?;
        self.tick = self
            .tick
            .checked_add(1)
            .ok_or_else(|| ZoneError::new("zone tick overflow"))?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<ZoneSnapshot, ZoneError> {
        let players = self
            .players
            .keys()
            .copied()
            .map(|player_id| self.player_snapshot(player_id))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.make_snapshot(players))
    }

    pub fn snapshot_for_player(&self, player_id: PlayerId) -> Result<ZoneSnapshot, ZoneError> {
        if !self.players.contains_key(&player_id) {
            return Err(ZoneError::new("unknown player"));
        }
        let center = self
            .world
            .body(Self::body_id(player_id))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?
            .position();

        let radius = i64::from(INTEREST_RADIUS_UNITS);
        let radius_squared = radius * radius;
        let players = self
            .players
            .keys()
            .copied()
            .filter_map(|candidate| {
                let snapshot = self.player_snapshot(candidate).ok()?;
                let dx = i64::from(snapshot.position[0]) - i64::from(center.x);
                let dz = i64::from(snapshot.position[2]) - i64::from(center.z);
                ((dx * dx) + (dz * dz) <= radius_squared).then_some(snapshot)
            })
            .collect();

        Ok(self.make_snapshot(players))
    }

    fn make_snapshot(&self, players: Vec<PlayerSnapshot>) -> ZoneSnapshot {
        ZoneSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: self.zone_id,
            tick: self.tick,
            players,
        }
    }

    fn player_snapshot(&self, player_id: PlayerId) -> Result<PlayerSnapshot, ZoneError> {
        let position = self
            .world
            .body(Self::body_id(player_id))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?
            .position();
        Ok(PlayerSnapshot {
            player_id,
            position: [position.x, position.y, position.z],
        })
    }

    fn body_id(player_id: PlayerId) -> BodyId {
        BodyId(PLAYER_BODY_BASE + u64::from(player_id))
    }

    fn spawn_position(player_id: PlayerId) -> Vec3i {
        let column = i32::try_from(player_id % SPAWN_GRID_WIDTH)
            .expect("spawn grid column always fits in i32");
        let row = i32::try_from((player_id / SPAWN_GRID_WIDTH) % SPAWN_GRID_WIDTH)
            .expect("spawn grid row always fits in i32");
        Vec3i::new(column * SPAWN_SPACING, 50, row * SPAWN_SPACING)
    }

    fn movement_velocity(state: PlayerState) -> Vec3i {
        let moving_diagonally = state.movement_x != 0 && state.movement_z != 0;
        let speed = if moving_diagonally {
            PLAYER_DIAGONAL_SPEED
        } else {
            PLAYER_SPEED
        };
        Vec3i::new(
            i32::from(state.movement_x) * speed,
            0,
            i32::from(state.movement_z) * speed,
        )
    }
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

        zone.apply_command(1, 1, ZoneCommand::SetMovement { x: 1, z: 0 })
            .unwrap();
        for _ in 0..10 {
            zone.advance_tick().unwrap();
        }

        let after = zone.snapshot().unwrap().players[0].position;
        assert!(after[0] > before[0]);
        assert_eq!(after[2], before[2]);
    }

    #[test]
    fn stale_commands_fail_closed() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        zone.apply_command(1, 5, ZoneCommand::SetMovement { x: 1, z: 0 })
            .unwrap();

        let error = zone
            .apply_command(1, 5, ZoneCommand::SetMovement { x: -1, z: 0 })
            .unwrap_err();

        assert_eq!(error.message(), "command sequence is stale");
    }

    #[test]
    fn player_snapshot_applies_zone_owned_interest_policy() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        zone.add_player(15).unwrap();

        let canonical = zone.snapshot().unwrap();
        let visible = zone.snapshot_for_player(1).unwrap();

        assert_eq!(canonical.players.len(), 2);
        assert_eq!(visible.players.len(), 1);
        assert_eq!(visible.players[0].player_id, 1);
    }
}
