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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneBox {
    pub position: [f32; 3],
    pub size: [f32; 3],
    pub color: [f32; 3],
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

    /// Two ticks of interpolation delay; packet loss holds the latest position.
    #[must_use]
    pub fn players(&self, now: Instant) -> BTreeMap<u32, [f32; 3]> {
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
                        let position = metres(player.position);
                        let next = visible_players(after).find(|next| next.id == player.id);
                        let position = next.map_or(position, |next| {
                            let target = metres(next.position);
                            std::array::from_fn(|axis| {
                                position[axis] + (target[axis] - position[axis]) * alpha
                            })
                        });
                        (player.id, position)
                    })
                    .collect();
            }
            before = after;
        }
        visible_players(before)
            .map(|player| (player.id, metres(player.position)))
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
            })
            .collect();
        boxes.extend(
            self.players(now)
                .into_iter()
                .map(|(id, position)| SceneBox {
                    position,
                    size: metres(PLAYER_HALF_EXTENTS_UNITS).map(|value| value * 2.0),
                    color: if id == self.player_id {
                        [0.95, 0.75, 0.25]
                    } else {
                        [0.3, 0.6, 0.95]
                    },
                }),
        );
        boxes
    }

    #[must_use]
    pub fn camera_target(&self, now: Instant) -> [f32; 3] {
        self.players(now)
            .get(&self.player_id)
            .copied()
            .unwrap_or([0.0, 0.5, 0.0])
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

fn metres(value: [i32; 3]) -> [f32; 3] {
    value.map(|component| component as f32 / UNITS_PER_METRE as f32)
}
