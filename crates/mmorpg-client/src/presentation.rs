//! Snapshot-only presentation. Local input never changes authoritative positions.
use crate::ClientError;
use mmorpg_core::{
    EntityKind, EntitySnapshot, PLAYER_HALF_EXTENTS_UNITS, TICK_HZ, UNITS_PER_METRE,
    ZoneDefinition, ZoneSnapshot,
};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

/// Nose marker in metres: small enough to read as facing, not as a collider.
const NOSE_SIZE: [f32; 3] = [0.14, 0.14, 0.2];
/// The nose sits at head height, just in front of the body's face.
const NOSE_HEIGHT_ABOVE_CENTER: f32 = 0.55;

/// A box rotated by `yaw` radians about +Y (0 keeps local +Z on world +Z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneBox {
    pub position: [f32; 3],
    pub size: [f32; 3],
    pub color: [f32; 3],
    pub yaw: f32,
}

/// Interpolated presentation pose of one player, in metres and radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerPose {
    pub position: [f32; 3],
    /// World yaw convention (0 faces +Z, increasing toward +X), within one turn.
    pub yaw: f32,
}

pub struct Presentation {
    player_id: u32,
    definition: ZoneDefinition,
    history: VecDeque<ZoneSnapshot>,
    latest_received: Instant,
}

impl Presentation {
    #[must_use]
    pub fn new(player_id: u32, definition: ZoneDefinition, now: Instant) -> Self {
        Self {
            player_id,
            definition,
            history: VecDeque::new(),
            latest_received: now,
        }
    }

    /// Start a new connection epoch without blending with obsolete samples.
    pub fn reset(&mut self, now: Instant) {
        self.history.clear();
        self.latest_received = now;
    }

    pub fn push(&mut self, snapshot: ZoneSnapshot, now: Instant) -> Result<bool, ClientError> {
        if snapshot.content_revision != self.definition.revision() {
            return Err("snapshot content revision mismatch".into());
        }
        if snapshot.viewer_id != self.player_id {
            return Err("snapshot is addressed to another player".into());
        }
        let mut entities = std::collections::BTreeSet::new();
        if snapshot
            .entities
            .iter()
            .any(|entity| !entities.insert((entity.kind, entity.id)))
        {
            return Err("duplicate entity in snapshot".into());
        }
        if let Some(latest) = self.history.back() {
            if snapshot.zone_id != latest.zone_id {
                return Err("zone changed without resetting presentation".into());
            }
            if snapshot.tick <= latest.tick {
                return Ok(false);
            }
            if snapshot.tick - latest.tick > u64::from(TICK_HZ) * 30 {
                self.history.clear();
            }
        }
        self.latest_received = now;
        self.history.push_back(snapshot);
        if self.history.len() > 32 {
            self.history.pop_front();
        }
        Ok(true)
    }

    /// Two ticks of interpolation delay; packet loss holds the latest pose.
    /// Facing interpolates along the shorter arc.
    #[must_use]
    pub fn players(&self, now: Instant) -> BTreeMap<u32, PlayerPose> {
        let Some(latest) = self.history.back() else {
            return BTreeMap::new();
        };
        let elapsed = now
            .saturating_duration_since(self.latest_received)
            .as_secs_f64()
            * f64::from(TICK_HZ);
        let delay = (2.0 - elapsed).max(0.0);
        let mut before = &self.history[0];
        for after in self.history.iter().skip(1) {
            // Subtract integer ticks before conversion, preserving precision at u64::MAX.
            let after_age = (latest.tick - after.tick) as f64;
            if after_age < delay {
                let before_age = (latest.tick - before.tick) as f64;
                let alpha =
                    ((before_age - delay) / (before_age - after_age)).clamp(0.0, 1.0) as f32;
                return visible_players(before)
                    .map(|player| {
                        let pose = pose(player);
                        let next = visible_players(after).find(|next| next.id == player.id);
                        let pose = next.map_or(pose, |next| {
                            let target = metres(next.position);
                            // Wrapping u16 difference, read as signed, is the shorter arc.
                            let arc = f32::from(next.facing.wrapping_sub(player.facing) as i16);
                            PlayerPose {
                                position: std::array::from_fn(|axis| {
                                    pose.position[axis]
                                        + (target[axis] - pose.position[axis]) * alpha
                                }),
                                yaw: yaw_radians(f32::from(player.facing) + arc * alpha),
                            }
                        });
                        (player.id, pose)
                    })
                    .collect();
            }
            before = after;
        }
        visible_players(before)
            .map(|player| (player.id, pose(player)))
            .collect()
    }

    #[must_use]
    pub fn scene(&self, now: Instant) -> Vec<SceneBox> {
        let mut boxes: Vec<_> = self
            .definition
            .colliders()
            .iter()
            .map(|collider| SceneBox {
                position: metres(collider.position),
                size: metres(collider.half_extents).map(|value| value * 2.0),
                color: match collider.id {
                    1 => [0.24, 0.37, 0.25],
                    2 | 3 => [0.51, 0.32, 0.20],
                    5 => [0.2, 0.7, 0.65],
                    _ => [0.5, 0.49, 0.43],
                },
                yaw: 0.0,
            })
            .collect();
        let body = metres(PLAYER_HALF_EXTENTS_UNITS).map(|value| value * 2.0);
        for (id, pose) in self.players(now) {
            let color = if id == self.player_id {
                [0.95, 0.75, 0.25]
            } else {
                [0.3, 0.6, 0.95]
            };
            boxes.push(SceneBox {
                position: pose.position,
                size: body,
                color,
                yaw: pose.yaw,
            });
            // The nose touches the body's front face: half depth plus half nose depth.
            let reach = (body[2] + NOSE_SIZE[2]) / 2.0;
            boxes.push(SceneBox {
                position: [
                    pose.position[0] + pose.yaw.sin() * reach,
                    pose.position[1] + NOSE_HEIGHT_ABOVE_CENTER,
                    pose.position[2] + pose.yaw.cos() * reach,
                ],
                size: NOSE_SIZE,
                color: color.map(|channel| channel * 0.45),
                yaw: pose.yaw,
            });
        }
        boxes
    }

    #[must_use]
    pub fn camera_target(&self, now: Instant) -> [f32; 3] {
        self.players(now)
            .get(&self.player_id)
            .map_or([0.0, 0.5, 0.0], |pose| pose.position)
    }

    #[must_use]
    pub fn is_stalled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.latest_received) > Duration::from_secs(1)
    }
}

fn visible_players(snapshot: &ZoneSnapshot) -> impl Iterator<Item = &EntitySnapshot> {
    snapshot
        .entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::Player)
}

/// Converts a presentation angle (radians, 0 facing +Z, turning toward +X)
/// into the nearest `u16` yaw intent.
#[must_use]
pub fn yaw_from_radians(radians: f32) -> u16 {
    let turns = (radians / std::f32::consts::TAU).rem_euclid(1.0);
    // A full turn rounds to 65 536, which wraps to yaw 0.
    ((turns * 65_536.0).round() as u32 % 65_536) as u16
}

fn pose(player: &EntitySnapshot) -> PlayerPose {
    PlayerPose {
        position: metres(player.position),
        yaw: yaw_radians(f32::from(player.facing)),
    }
}

/// Converts yaw steps (possibly fractional or outside one turn) to radians within one turn.
fn yaw_radians(steps: f32) -> f32 {
    (steps / 65_536.0).rem_euclid(1.0) * std::f32::consts::TAU
}

fn metres(value: [i32; 3]) -> [f32; 3] {
    value.map(|component| component as f32 / UNITS_PER_METRE as f32)
}
