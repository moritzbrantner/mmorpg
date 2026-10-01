//! Snapshot-only presentation. Local input never changes authoritative positions.
//! Units stand on the shared presentation relief: a rendered position is the
//! physics position plus `Scenery::height_at` at its XZ, never fed back.
//!
//! Players render as a character box, creatures as a box of their template's
//! collision size and NPCs as a character-sized post, each with a facing
//! marker. Creatures are coloured by disposition (hostile red-ish, neutral
//! yellow-ish) and shaded by family, NPCs green-ish; corpses lie flat, and
//! the viewer's target stands on a marker. Names come from the zone content.
use crate::{ClientError, camera::CameraView};
use mmorpg_core::{
    CreatureFamily, CreatureTemplateId, EntityKind, EntityRef, EntitySnapshot, NpcRole,
    PLAYER_HALF_EXTENTS_UNITS, TICK_HZ, UNITS_PER_METRE, ZoneContent, ZoneSnapshot,
};
use mmorpg_scenery::Scenery;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

/// Nose marker in metres: small enough to read as facing, not as a collider.
const NOSE_SIZE: [f32; 3] = [0.14, 0.14, 0.2];
/// The nose sits at head height, just in front of the body's face.
const NOSE_HEIGHT_ABOVE_CENTER: f32 = 0.55;
const SELF_COLOR: [f32; 3] = [0.95, 0.75, 0.25];
const PLAYER_COLOR: [f32; 3] = [0.3, 0.6, 0.95];
const FRIENDLY_COLOR: [f32; 3] = [0.3, 0.62, 0.36];
const GUARD_COLOR: [f32; 3] = [0.22, 0.52, 0.3];
const TAPPED_COLOR: [f32; 3] = [0.5, 0.5, 0.5];
const TARGET_MARKER_COLOR: [f32; 3] = [0.95, 0.82, 0.35];
/// Corpses keep their colour at this brightness.
const CORPSE_SHADE: f32 = 0.45;
/// A corpse lies flat at this thickness.
const CORPSE_HEIGHT: f32 = 0.16;
const HEALTH_BAR_WIDTH: f32 = 1.0;
const HEALTH_BAR_BACKGROUND: [f32; 3] = [0.12, 0.12, 0.14];
const HEALTH_BAR_FILL: [f32; 3] = [0.18, 0.9, 0.25];

/// Body, nose and two health-bar boxes per projected unit, plus one target marker.
pub const MAX_SCENE_BOXES: usize = 4 * mmorpg_core::MAX_VISIBLE_ENTITIES + 1;

/// A box rotated by `yaw` radians about +Y (0 keeps local +Z on world +Z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneBox {
    pub position: [f32; 3],
    pub size: [f32; 3],
    pub color: [f32; 3],
    pub yaw: f32,
}

/// Interpolated presentation pose of one unit, in metres and radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerPose {
    /// Physics position lifted by the relief height at its XZ.
    pub position: [f32; 3],
    /// World yaw convention (0 faces +Z, increasing toward +X), within one turn.
    pub yaw: f32,
}

/// An interpolated unit with the record it was sampled from.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitPose {
    pub pose: PlayerPose,
    pub record: EntitySnapshot,
}

pub struct Presentation {
    player_id: u32,
    scenery: Scenery,
    content: Arc<ZoneContent>,
    history: VecDeque<ZoneSnapshot>,
    latest_received: Instant,
}

impl Presentation {
    /// Presents snapshots of the content revision the scenery was built from;
    /// `content` names and sizes the units of that revision.
    pub fn new(
        player_id: u32,
        scenery: Scenery,
        content: Arc<ZoneContent>,
        now: Instant,
    ) -> Result<Self, ClientError> {
        if content.revision() != scenery.content_revision {
            return Err("scenery and zone content revisions differ".into());
        }
        Ok(Self {
            player_id,
            scenery,
            content,
            history: VecDeque::new(),
            latest_received: now,
        })
    }

    /// Start a new connection epoch without blending with obsolete samples.
    pub fn reset(&mut self, now: Instant) {
        self.history.clear();
        self.latest_received = now;
    }

    pub fn push(&mut self, snapshot: ZoneSnapshot, now: Instant) -> Result<bool, ClientError> {
        if snapshot.content_revision != self.scenery.content_revision {
            return Err("snapshot content revision mismatch".into());
        }
        if snapshot.viewer_id != self.player_id {
            return Err("snapshot is addressed to another player".into());
        }
        if snapshot.entities.len() > mmorpg_core::MAX_VISIBLE_ENTITIES {
            return Err("snapshot exceeds visible entity capacity".into());
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

    /// The newest projection, for input decisions and status text.
    #[must_use]
    pub fn latest(&self) -> Option<&ZoneSnapshot> {
        self.history.back()
    }

    /// Two ticks of interpolation delay; packet loss holds the latest pose.
    /// Facing interpolates along the shorter arc. Units appear and vanish at
    /// sample ticks.
    #[must_use]
    pub fn units(&self, now: Instant) -> BTreeMap<EntityRef, UnitPose> {
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
                    .entities
                    .iter()
                    .map(|unit| {
                        let pose = self.pose(unit);
                        let next = after
                            .entities
                            .iter()
                            .find(|next| (next.kind, next.id) == (unit.kind, unit.id));
                        let pose = next.map_or(pose, |next| {
                            let target = self.rendered(next.position);
                            // Wrapping u16 difference, read as signed, is the shorter arc.
                            let arc = f32::from(next.facing.wrapping_sub(unit.facing) as i16);
                            PlayerPose {
                                position: std::array::from_fn(|axis| {
                                    pose.position[axis]
                                        + (target[axis] - pose.position[axis]) * alpha
                                }),
                                yaw: yaw_radians(f32::from(unit.facing) + arc * alpha),
                            }
                        });
                        (
                            unit.entity(),
                            UnitPose {
                                pose,
                                record: unit.clone(),
                            },
                        )
                    })
                    .collect();
            }
            before = after;
        }
        before
            .entities
            .iter()
            .map(|unit| {
                (
                    unit.entity(),
                    UnitPose {
                        pose: self.pose(unit),
                        record: unit.clone(),
                    },
                )
            })
            .collect()
    }

    /// Interpolated poses of the visible players.
    #[must_use]
    pub fn players(&self, now: Instant) -> BTreeMap<u32, PlayerPose> {
        self.units(now)
            .into_iter()
            .filter_map(|(entity, unit)| match entity {
                EntityRef::Player(id) => Some((id, unit.pose)),
                EntityRef::Creature(_) | EntityRef::Npc(_) => None,
            })
            .collect()
    }

    /// Per-frame bodies, facing markers and living-unit health bars,
    /// plus a marker under the viewer's target. Static scenery is uploaded
    /// once (`world::WorldScene`).
    #[must_use]
    pub fn scene(&self, now: Instant, view: CameraView) -> Vec<SceneBox> {
        let target = self.latest().and_then(|latest| latest.viewer.target);
        let bar_yaw = (view.target[0] - view.eye[0]).atan2(view.target[2] - view.eye[2]);
        let mut boxes = Vec::new();
        for (entity, unit) in self.units(now) {
            let (color, half) = self.unit_look(entity, &unit.record);
            let size = half.map(|value| value * 2.0);
            let pose = unit.pose;
            // Rendered positions are body centres; the feet are half a body lower.
            let feet = pose.position[1] - half[1];
            if target == Some(entity) {
                boxes.push(SceneBox {
                    position: [pose.position[0], feet + 0.02, pose.position[2]],
                    size: [size[0] + 0.7, 0.04, size[2] + 0.7],
                    color: TARGET_MARKER_COLOR,
                    yaw: pose.yaw,
                });
            }
            if unit.record.flags.dead && entity.kind() == EntityKind::Creature {
                // A corpse lies flat: its height becomes its length.
                boxes.push(SceneBox {
                    position: [
                        pose.position[0],
                        feet + CORPSE_HEIGHT / 2.0,
                        pose.position[2],
                    ],
                    size: [size[0], CORPSE_HEIGHT, size[1]],
                    color: color.map(|channel| channel * CORPSE_SHADE),
                    yaw: pose.yaw,
                });
                continue;
            }
            boxes.push(SceneBox {
                position: pose.position,
                size,
                color,
                yaw: pose.yaw,
            });
            // The nose touches the body's front face: half depth plus half nose depth.
            let reach = (size[2] + NOSE_SIZE[2]) / 2.0;
            let height = if entity.kind() == EntityKind::Creature {
                half[1] * 0.5
            } else {
                NOSE_HEIGHT_ABOVE_CENTER
            };
            boxes.push(SceneBox {
                position: [
                    pose.position[0] + pose.yaw.sin() * reach,
                    pose.position[1] + height,
                    pose.position[2] + pose.yaw.cos() * reach,
                ],
                size: NOSE_SIZE,
                color: color.map(|channel| channel * 0.45),
                yaw: pose.yaw,
            });
            if !unit.record.flags.dead && entity.kind() != EntityKind::Npc {
                append_health_bar(
                    &mut boxes,
                    [
                        pose.position[0],
                        pose.position[1] + half[1] + 0.3,
                        pose.position[2],
                    ],
                    unit.record.health_percent,
                    bar_yaw,
                );
            }
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

    /// Tab targeting: the nearest living attackable creature in the latest
    /// projection; while one of them is selected, the next one outward.
    #[must_use]
    pub fn next_tab_target(&self) -> Option<EntityRef> {
        let latest = self.latest()?;
        let viewer = latest.entities.first()?;
        let mut candidates: Vec<_> = latest
            .entities
            .iter()
            .filter(|entity| entity.kind == EntityKind::Creature && entity.flags.attackable)
            .map(|entity| {
                let dx = i64::from(entity.position[0] - viewer.position[0]);
                let dz = i64::from(entity.position[2] - viewer.position[2]);
                (dx * dx + dz * dz, entity.entity())
            })
            .collect();
        candidates.sort_unstable();
        let current = candidates
            .iter()
            .position(|(_, entity)| Some(*entity) == latest.viewer.target);
        let next = current.map_or(0, |index| (index + 1) % candidates.len());
        candidates.get(next).map(|(_, entity)| *entity)
    }

    /// A unit's display name from the zone content; "you" for the viewer.
    #[must_use]
    pub fn unit_name(&self, entity: EntityRef) -> String {
        match entity {
            EntityRef::Player(id) if id == self.player_id => "you".into(),
            EntityRef::Player(id) => format!("Player {id}"),
            EntityRef::Npc(id) => self
                .content
                .npc(id)
                .map_or_else(|| format!("NPC {}", id.get()), |npc| npc.name.clone()),
            EntityRef::Creature(_) => self
                .latest()
                .and_then(|latest| latest.entities.iter().find(|unit| unit.entity() == entity))
                .and_then(|unit| {
                    self.content
                        .creature_template(CreatureTemplateId::new(unit.appearance))
                })
                .map_or_else(|| "a creature".into(), |template| template.name.clone()),
        }
    }

    /// The viewer's health, combat state and target, or how to release a
    /// dead spirit; `None` before the first projection.
    #[must_use]
    pub fn status(&self) -> Option<String> {
        let latest = self.latest()?;
        let viewer = latest.viewer;
        if viewer.dead {
            return Some("dead — R releases your spirit".into());
        }
        let mut parts = vec![format!("HP {}/{}", viewer.health, viewer.max_health)];
        if viewer.in_combat {
            parts.push("in combat".into());
        }
        if let Some(target) = viewer.target {
            let detail = latest
                .entities
                .iter()
                .find(|unit| unit.entity() == target)
                .map_or_else(
                    || "out of sight".into(),
                    |unit| {
                        if unit.flags.dead {
                            format!("L{}, dead", unit.level)
                        } else {
                            format!("L{}, {}%", unit.level, unit.health_percent)
                        }
                    },
                );
            parts.push(format!("target {} ({detail})", self.unit_name(target)));
        }
        if viewer.auto_attacking {
            parts.push("attacking".into());
        }
        Some(parts.join(" · "))
    }

    /// Colour and half extents in metres of a unit.
    fn unit_look(&self, entity: EntityRef, record: &EntitySnapshot) -> ([f32; 3], [f32; 3]) {
        let character = metres(PLAYER_HALF_EXTENTS_UNITS);
        match entity {
            EntityRef::Player(id) if id == self.player_id => (SELF_COLOR, character),
            EntityRef::Player(_) => (PLAYER_COLOR, character),
            EntityRef::Npc(id) => {
                let guard = self
                    .content
                    .npc(id)
                    .is_some_and(|npc| npc.role == NpcRole::Guard);
                (if guard { GUARD_COLOR } else { FRIENDLY_COLOR }, character)
            }
            EntityRef::Creature(_) => {
                let template = self
                    .content
                    .creature_template(CreatureTemplateId::new(record.appearance));
                let half = template.map_or(character, |template| metres(template.half_extents));
                let family = template.map_or(CreatureFamily::Wolf, |template| template.family);
                let color = if record.flags.tapped_by_other {
                    TAPPED_COLOR
                } else {
                    creature_color(family, record.flags.hostile)
                };
                (color, half)
            }
        }
    }

    fn pose(&self, unit: &EntitySnapshot) -> PlayerPose {
        PlayerPose {
            position: self.rendered(unit.position),
            yaw: yaw_radians(f32::from(unit.facing)),
        }
    }

    /// Metres, lifted by the presentation relief under the unit.
    fn rendered(&self, position: [i32; 3]) -> [f32; 3] {
        let relief = self.scenery.height_at(position[0], position[2]);
        let [x, y, z] = metres(position);
        [x, y + relief as f32 / UNITS_PER_METRE as f32, z]
    }
}

fn append_health_bar(boxes: &mut Vec<SceneBox>, position: [f32; 3], percent: u8, yaw: f32) {
    boxes.push(SceneBox {
        position,
        size: [HEALTH_BAR_WIDTH + 0.04, 0.12, 0.04],
        color: HEALTH_BAR_BACKGROUND,
        yaw,
    });
    let width = HEALTH_BAR_WIDTH * f32::from(percent.min(100)) / 100.0;
    if width > 0.0 {
        // Local +X is screen-left. Shift the fill toward that edge, and
        // toward the eye (-local Z) so the background cannot obscure it.
        let left = (HEALTH_BAR_WIDTH - width) / 2.0;
        let front = -0.035;
        boxes.push(SceneBox {
            position: [
                position[0] + left * yaw.cos() + front * yaw.sin(),
                position[1],
                position[2] + front * yaw.cos() - left * yaw.sin(),
            ],
            size: [width, 0.08, 0.02],
            color: HEALTH_BAR_FILL,
            yaw,
        });
    }
}

/// Hostile creatures red-ish, neutral ones yellow-ish, shaded per family.
fn creature_color(family: CreatureFamily, hostile: bool) -> [f32; 3] {
    let shade = match family {
        CreatureFamily::Wolf => 0.0,
        CreatureFamily::Boar => 0.04,
        CreatureFamily::Vermin => 0.08,
        CreatureFamily::Marauder => 0.12,
        CreatureFamily::Mirefin => 0.16,
        CreatureFamily::Redbrand => 0.2,
    };
    if hostile {
        [0.78 - shade, 0.24 + shade * 0.5, 0.18]
    } else {
        [0.8 - shade * 0.5, 0.68 - shade, 0.2]
    }
}

/// Converts a presentation angle (radians, 0 facing +Z, turning toward +X)
/// into the nearest `u16` yaw intent.
#[must_use]
pub fn yaw_from_radians(radians: f32) -> u16 {
    let turns = (radians / std::f32::consts::TAU).rem_euclid(1.0);
    // A full turn rounds to 65 536, which wraps to yaw 0.
    ((turns * 65_536.0).round() as u32 % 65_536) as u16
}

/// Converts yaw steps (possibly fractional or outside one turn) to radians within one turn.
fn yaw_radians(steps: f32) -> f32 {
    (steps / 65_536.0).rem_euclid(1.0) * std::f32::consts::TAU
}

fn metres(value: [i32; 3]) -> [f32; 3] {
    value.map(|component| component as f32 / UNITS_PER_METRE as f32)
}
