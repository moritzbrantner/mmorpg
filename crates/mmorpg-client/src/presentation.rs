//! Snapshot-only presentation. Local input never changes authoritative positions.
use crate::ClientError;
use mmorpg_core::{
    PLAYER_HALF_EXTENTS_UNITS, TICK_HZ, UNITS_PER_METRE, ZoneDefinition, ZoneSnapshot,
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

    pub fn push(&mut self, snapshot: ZoneSnapshot, now: Instant) -> Result<bool, ClientError> {
        if snapshot.content_revision != self.definition.revision() {
            return Err("snapshot content revision mismatch".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        if snapshot
            .players
            .iter()
            .any(|player| !ids.insert(player.player_id))
        {
            return Err("duplicate player in snapshot".into());
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
                return before
                    .players
                    .iter()
                    .map(|player| {
                        let position = metres(player.position);
                        let next = after
                            .players
                            .iter()
                            .find(|next| next.player_id == player.player_id);
                        let position = next.map_or(position, |next| {
                            let target = metres(next.position);
                            std::array::from_fn(|axis| {
                                position[axis] + (target[axis] - position[axis]) * alpha
                            })
                        });
                        (player.player_id, position)
                    })
                    .collect();
            }
            before = after;
        }
        before
            .players
            .iter()
            .map(|player| (player.player_id, metres(player.position)))
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

fn metres(value: [i32; 3]) -> [f32; 3] {
    value.map(|component| component as f32 / UNITS_PER_METRE as f32)
}
