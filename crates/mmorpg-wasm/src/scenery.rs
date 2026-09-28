//! Versioned presentation scenery for the browser's `SceneryProvider`.
//!
//! The export is JSON (format [`SCENERY_FORMAT`], version
//! [`SCENERY_FORMAT_VERSION`]) so `mmorpg-core` stays free of serde. It
//! describes a terrain grid, props, water and the core's named areas.
//!
//! `mmorpg-scenery` is not part of the workspace yet, so this module derives
//! a **collider blockout** from the hosted `ZoneDefinition`: a flat terrain
//! grid around the colliders and one coloured box per collider standing
//! above walkable ground. Colliders whose top is at or below `y = 0` are
//! ground support and appear as the terrain's courtyard biome instead. When
//! the scenery crate lands, it replaces this mapping behind the same format;
//! the browser's render loop does not change. None of this is gameplay: it
//! only decides what the static world looks like.

use std::sync::OnceLock;

use mmorpg_core::{StaticCollider, UNITS_PER_METRE, ZoneAreas, ZoneDefinition, outpost_areas};
use serde::Serialize;

use crate::host::hosted_definition;

pub const SCENERY_FORMAT: &str = "mmorpg.scenery";
pub const SCENERY_FORMAT_VERSION: u32 = 1;
/// Where this export's props and terrain come from.
pub const SCENERY_SOURCE: &str = "core-collider-blockout";

/// Terrain sample spacing: 4 m.
const TERRAIN_STEP_UNITS: i32 = 400;
/// Terrain extends this far beyond the outermost collider: 40 m.
const TERRAIN_MARGIN_UNITS: i32 = 4_000;
/// A collider this much longer than it is thick reads as a wall.
const WALL_ASPECT: i32 = 8;

const BIOME_WILDS: u8 = 0;
const BIOME_COURTYARD: u8 = 1;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneryExport {
    pub format: &'static str,
    pub version: u32,
    pub source: &'static str,
    /// Decimal `u64`, so JavaScript never rounds it.
    pub content_revision: String,
    pub units_per_metre: i32,
    pub terrain: Terrain,
    pub biomes: Vec<Biome>,
    pub props: Vec<Prop>,
    pub water: Vec<Water>,
    pub areas: Vec<AreaExport>,
}

/// A regular height grid. Sample `(column, row)` lies at
/// `origin_xz + step × (column, row)`; arrays are row-major (Z outer).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Terrain {
    pub origin_xz: [i32; 2],
    pub step: i32,
    pub columns: u32,
    pub rows: u32,
    /// Presentation height per sample in units.
    pub heights: Vec<i32>,
    /// Biome ID per sample.
    pub biomes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Biome {
    pub id: u8,
    pub name: &'static str,
    pub color: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prop {
    /// `block`: a box of `half_extents`. Later formats add named kinds.
    pub kind: &'static str,
    /// The collider this prop visualises, if any.
    pub collider_id: Option<u32>,
    /// Centre in units.
    pub position: [i32; 3],
    /// `u16` yaw: 0 faces +Z, increasing toward +X.
    pub yaw: u16,
    pub half_extents: [i32; 3],
    pub color: &'static str,
}

/// A flat elliptical water surface.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Water {
    pub center_xz: [i32; 2],
    pub radii_xz: [i32; 2],
    pub surface_y: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaExport {
    pub id: u16,
    pub name: String,
    pub min_xz: [i32; 2],
    pub max_xz: [i32; 2],
}

/// The areas of the hosted content, built once.
pub fn hosted_areas() -> &'static ZoneAreas {
    static AREAS: OnceLock<ZoneAreas> = OnceLock::new();
    AREAS.get_or_init(outpost_areas)
}

/// Presentation relief at `(x, z)` in units. Clients offset units by it
/// vertically; the blockout is flat. `mmorpg-scenery`'s shared relief
/// function replaces this.
#[must_use]
pub const fn relief_at(_x: i32, _z: i32) -> i32 {
    0
}

#[must_use]
pub fn hosted_scenery() -> SceneryExport {
    blockout(&hosted_definition(), hosted_areas())
}

/// The serialized export. Serializing plain data cannot fail.
#[must_use]
pub fn hosted_scenery_json() -> &'static str {
    static JSON: OnceLock<String> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::to_string(&hosted_scenery()).expect("scenery export is plain serializable data")
    })
}

fn blockout(definition: &ZoneDefinition, areas: &ZoneAreas) -> SceneryExport {
    let (ground, standing): (Vec<_>, Vec<_>) = definition
        .colliders()
        .iter()
        .partition(|collider| top(collider) <= 0);
    SceneryExport {
        format: SCENERY_FORMAT,
        version: SCENERY_FORMAT_VERSION,
        source: SCENERY_SOURCE,
        content_revision: definition.revision().to_string(),
        units_per_metre: UNITS_PER_METRE,
        terrain: terrain(definition.colliders(), &ground),
        biomes: vec![
            Biome {
                id: BIOME_WILDS,
                name: "wilds",
                color: "#4c6b3f",
            },
            Biome {
                id: BIOME_COURTYARD,
                name: "courtyard",
                color: "#7d8a63",
            },
        ],
        props: standing.into_iter().map(block).collect(),
        water: Vec::new(),
        areas: areas
            .areas()
            .iter()
            .map(|area| AreaExport {
                id: area.id().get(),
                name: area.name().to_owned(),
                min_xz: area.min_xz(),
                max_xz: area.max_xz(),
            })
            .collect(),
    }
}

fn top(collider: &StaticCollider) -> i32 {
    collider.position[1].saturating_add(collider.half_extents[1])
}

fn block(collider: &StaticCollider) -> Prop {
    let [half_x, _, half_z] = collider.half_extents;
    let thin = half_x.min(half_z);
    let long = half_x.max(half_z);
    let color = if long >= thin.saturating_mul(WALL_ASPECT) {
        "#8b8375"
    } else {
        "#8a6446"
    };
    Prop {
        kind: "block",
        collider_id: Some(collider.id),
        position: collider.position,
        yaw: 0,
        half_extents: collider.half_extents,
        color,
    }
}

fn terrain(colliders: &[StaticCollider], ground: &[&StaticCollider]) -> Terrain {
    let (mut min, mut max) = ([0_i32; 2], [0_i32; 2]);
    for collider in colliders {
        for (axis, component) in [(0, 0), (1, 2)] {
            min[axis] = min[axis]
                .min(collider.position[component].saturating_sub(collider.half_extents[component]));
            max[axis] = max[axis]
                .max(collider.position[component].saturating_add(collider.half_extents[component]));
        }
    }
    let origin_xz = min.map(|value| snap_down(value.saturating_sub(TERRAIN_MARGIN_UNITS)));
    let far_xz = max.map(|value| snap_up(value.saturating_add(TERRAIN_MARGIN_UNITS)));
    let samples = |axis: usize| {
        u32::try_from((far_xz[axis] - origin_xz[axis]) / TERRAIN_STEP_UNITS + 1)
            .expect("terrain spans a positive sample count")
    };
    let (columns, rows) = (samples(0), samples(1));
    let mut heights = Vec::new();
    let mut biomes = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            let x = origin_xz[0] + offset(column);
            let z = origin_xz[1] + offset(row);
            let supported = ground.iter().any(|collider| {
                (x - collider.position[0]).abs() <= collider.half_extents[0]
                    && (z - collider.position[2]).abs() <= collider.half_extents[2]
            });
            heights.push(relief_at(x, z));
            biomes.push(if supported {
                BIOME_COURTYARD
            } else {
                BIOME_WILDS
            });
        }
    }
    Terrain {
        origin_xz,
        step: TERRAIN_STEP_UNITS,
        columns,
        rows,
        heights,
        biomes,
    }
}

fn offset(samples: u32) -> i32 {
    i32::try_from(samples).expect("terrain sample count fits i32") * TERRAIN_STEP_UNITS
}

fn snap_down(value: i32) -> i32 {
    value.div_euclid(TERRAIN_STEP_UNITS) * TERRAIN_STEP_UNITS
}

fn snap_up(value: i32) -> i32 {
    snap_down(value)
        + if value.rem_euclid(TERRAIN_STEP_UNITS) == 0 {
            0
        } else {
            TERRAIN_STEP_UNITS
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn every_standing_collider_has_exactly_one_block_and_ground_is_terrain() {
        let definition = hosted_definition();
        let scenery = hosted_scenery();
        let mut visualised = scenery
            .props
            .iter()
            .map(|prop| prop.collider_id.unwrap())
            .collect::<Vec<_>>();
        visualised.sort_unstable();
        let standing = definition
            .colliders()
            .iter()
            .filter(|collider| top(collider) > 0)
            .map(|collider| collider.id)
            .collect::<Vec<_>>();
        assert_eq!(visualised, standing);
        assert!(
            definition
                .colliders()
                .iter()
                .any(|collider| top(collider) <= 0),
            "the outpost has a ground slab"
        );
        for prop in &scenery.props {
            let collider = definition
                .colliders()
                .iter()
                .find(|collider| Some(collider.id) == prop.collider_id)
                .unwrap();
            assert_eq!(prop.position, collider.position);
            assert_eq!(prop.half_extents, collider.half_extents);
        }
    }

    #[test]
    fn terrain_covers_every_collider_and_marks_supported_ground() {
        let scenery = hosted_scenery();
        let terrain = &scenery.terrain;
        let samples = usize::try_from(terrain.columns * terrain.rows).unwrap();
        assert_eq!(terrain.heights.len(), samples);
        assert_eq!(terrain.biomes.len(), samples);
        assert!(
            terrain.heights.iter().all(|height| *height == 0),
            "flat blockout"
        );
        let far = [
            terrain.origin_xz[0] + offset(terrain.columns - 1),
            terrain.origin_xz[1] + offset(terrain.rows - 1),
        ];
        for collider in hosted_definition().colliders() {
            assert!(collider.position[0] - collider.half_extents[0] >= terrain.origin_xz[0]);
            assert!(collider.position[2] - collider.half_extents[2] >= terrain.origin_xz[1]);
            assert!(collider.position[0] + collider.half_extents[0] <= far[0]);
            assert!(collider.position[2] + collider.half_extents[2] <= far[1]);
        }
        let biome_at = |x: i32, z: i32| {
            let column = u32::try_from((x - terrain.origin_xz[0]) / terrain.step).unwrap();
            let row = u32::try_from((z - terrain.origin_xz[1]) / terrain.step).unwrap();
            terrain.biomes[usize::try_from(row * terrain.columns + column).unwrap()]
        };
        assert_eq!(biome_at(0, 0), BIOME_COURTYARD, "spawn stands on the slab");
        assert_eq!(
            biome_at(terrain.origin_xz[0], terrain.origin_xz[1]),
            BIOME_WILDS
        );
    }

    #[test]
    fn export_is_versioned_deterministic_json_with_core_areas() {
        let json = hosted_scenery_json();
        assert_eq!(json, serde_json::to_string(&hosted_scenery()).unwrap());
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["format"], SCENERY_FORMAT);
        assert_eq!(value["version"], SCENERY_FORMAT_VERSION);
        assert_eq!(value["source"], SCENERY_SOURCE);
        assert_eq!(
            value["contentRevision"],
            hosted_definition().revision().to_string()
        );
        assert_eq!(value["unitsPerMetre"], 100);
        assert_eq!(value["areas"][0]["name"], "Greyhaven Outpost");
        assert_eq!(
            value["areas"][0]["minXz"],
            serde_json::json!([-1_100, -1_100])
        );
        assert_eq!(value["props"][0]["kind"], "block");
        assert!(value["water"].as_array().unwrap().is_empty());
    }

    #[test]
    fn walls_and_structures_get_distinct_blockout_colours() {
        let props = hosted_scenery().props;
        let colour = |id: u32| {
            props
                .iter()
                .find(|prop| prop.collider_id == Some(id))
                .unwrap()
                .color
        };
        assert_eq!(colour(6), colour(8), "boundary walls share a colour");
        assert_ne!(colour(2), colour(6), "buildings differ from walls");
    }

    #[test]
    fn snapping_encloses_values_on_the_sample_grid() {
        assert_eq!(snap_down(-1), -400);
        assert_eq!(snap_down(-400), -400);
        assert_eq!(snap_up(1), 400);
        assert_eq!(snap_up(400), 400);
        assert_eq!(snap_up(-399), 0);
    }
}
