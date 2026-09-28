//! Versioned presentation scenery for the browser's `SceneryProvider`.
//!
//! The export is JSON (format [`SCENERY_FORMAT`], version
//! [`SCENERY_FORMAT_VERSION`]) so `mmorpg-core` stays free of serde. It maps
//! the hosted content's `mmorpg-scenery` value, the same one the native
//! client draws, onto that format:
//!
//! - **terrain**: [`Scenery::terrain_grid`] every [`TERRAIN_STEP_UNITS`]
//!   over ±200 m, with one biome ID per sample. The browser renderer has no
//!   vertex colours, so each biome draws in its untinted base colour;
//! - **props**: one `block` per scenery prop, its body box standing on the
//!   relief at its feet, turned by the prop's yaw and coloured by its kind.
//!   A structure's block is exactly its core collider. The native client
//!   adds kind-specific parts (roofs, canopies) above the same bodies;
//! - **water** and the core's named **areas**.
//!
//! [`relief_at`] answers from the same `Scenery::height_at`, so browser and
//! native units stand on the same ground. None of this is gameplay: the
//! local zone host ([`crate::host`]) never reads it, and network hosts never
//! link `mmorpg-scenery`.

use std::sync::OnceLock;

use mmorpg_core::{PLAYER_HALF_EXTENTS_UNITS, UNITS_PER_METRE, ZoneAreas, greyhaven_vale};
use mmorpg_scenery::{PropKind, RockSize, Scenery, TreeVariant, greyhaven_vale_scenery};
use serde::Serialize;

pub const SCENERY_FORMAT: &str = "mmorpg.scenery";
pub const SCENERY_FORMAT_VERSION: u32 = 1;
/// Where this export's props and terrain come from.
pub const SCENERY_SOURCE: &str = "mmorpg-scenery";
/// Terrain sample spacing: 4 m, 101 × 101 samples over ±200 m.
pub const TERRAIN_STEP_UNITS: i32 = 400;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneryExport {
    pub format: &'static str,
    pub version: u32,
    pub source: &'static str,
    /// Decimal `u64`, so JavaScript never rounds it.
    pub content_revision: String,
    pub units_per_metre: i32,
    /// The player collision box from core, so clients place feet correctly.
    pub player_half_extents: [i32; 3],
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
    pub color: String,
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

/// The areas of the hosted content.
pub fn hosted_areas() -> &'static ZoneAreas {
    greyhaven_vale::areas()
}

/// The presentation scenery of the hosted content, built once.
pub fn hosted_scenery() -> &'static Scenery {
    static SCENERY: OnceLock<Scenery> = OnceLock::new();
    SCENERY.get_or_init(greyhaven_vale_scenery)
}

/// Presentation relief at `(x, z)` in units; clients raise units by it.
#[must_use]
pub fn relief_at(x: i32, z: i32) -> i32 {
    hosted_scenery().height_at(x, z)
}

#[must_use]
pub fn hosted_export() -> SceneryExport {
    export(hosted_scenery(), hosted_areas())
}

/// The serialized export. Serializing plain data cannot fail.
#[must_use]
pub fn hosted_scenery_json() -> &'static str {
    static JSON: OnceLock<String> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::to_string(&hosted_export()).expect("scenery export is plain serializable data")
    })
}

fn export(scenery: &Scenery, areas: &ZoneAreas) -> SceneryExport {
    SceneryExport {
        format: SCENERY_FORMAT,
        version: SCENERY_FORMAT_VERSION,
        source: SCENERY_SOURCE,
        content_revision: scenery.content_revision.to_string(),
        units_per_metre: UNITS_PER_METRE,
        player_half_extents: PLAYER_HALF_EXTENTS_UNITS,
        terrain: terrain(scenery),
        biomes: mmorpg_scenery::Biome::ALL
            .iter()
            .map(|biome| Biome {
                id: biome.id(),
                name: biome.name(),
                color: hex(biome.base_color()),
            })
            .collect(),
        props: scenery
            .props
            .iter()
            .map(|prop| block(scenery, prop))
            .collect(),
        water: scenery
            .water
            .iter()
            .map(|water| Water {
                center_xz: water.centre,
                radii_xz: water.radii,
                surface_y: water.surface,
            })
            .collect(),
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

fn terrain(scenery: &Scenery) -> Terrain {
    let grid = scenery.terrain_grid(TERRAIN_STEP_UNITS);
    let count = |samples: usize| u32::try_from(samples).expect("terrain sample count fits u32");
    Terrain {
        origin_xz: grid.origin,
        step: grid.step,
        columns: count(grid.columns),
        rows: count(grid.rows),
        heights: grid.heights,
        biomes: grid.biomes.iter().map(|biome| biome.id()).collect(),
    }
}

/// The prop's body box, standing on the relief at its feet.
fn block(scenery: &Scenery, prop: &mmorpg_scenery::Prop) -> Prop {
    let [x, feet, z] = prop.position;
    let base = feet + scenery.height_at(x, z);
    Prop {
        kind: "block",
        collider_id: prop.collider,
        position: [x, base + prop.half_extents[1], z],
        yaw: prop.yaw,
        half_extents: prop.half_extents,
        color: body_color(prop),
    }
}

// Body colours in sRGB: the native client's linear blockout body colours
// (`mmorpg-client/src/world.rs`), encoded for the browser renderer.
const STONE: &str = "#adaca6";
const DARK_STONE: &str = "#8e8b89";
const WOOD: &str = "#a28461";
const DARK_WOOD: &str = "#816950";
const PLASTER: &str = "#ddd4bf";
const FLOWERS: [&str; 4] = ["#f3e159", "#c489e1", "#f6f6f1", "#e76c61"];

fn body_color(prop: &mmorpg_scenery::Prop) -> &'static str {
    match prop.kind {
        PropKind::Keep
        | PropKind::Windmill
        | PropKind::Well
        | PropKind::Rock(RockSize::Small | RockSize::Medium) => STONE,
        PropKind::Smithy
        | PropKind::Gravestone
        | PropKind::Cliff
        | PropKind::Campfire
        | PropKind::Rock(RockSize::Large) => DARK_STONE,
        PropKind::Inn | PropKind::House | PropKind::Farmhouse => PLASTER,
        PropKind::PalisadeSegment
        | PropKind::Fence
        | PropKind::Cart
        | PropKind::Signpost
        | PropKind::MineEntrance
        | PropKind::Dock
        | PropKind::Tree(TreeVariant::Oak | TreeVariant::Pine) => WOOD,
        PropKind::GatePost | PropKind::Barrel | PropKind::Lamp => DARK_WOOD,
        PropKind::Barn => "#b35945",
        PropKind::Waystone => "#959eaa",
        PropKind::Tree(TreeVariant::Birch) => "#e1dfd7",
        PropKind::Bush => "#598b4b",
        PropKind::GrassTuft => "#6f9e4b",
        PropKind::Flowers => FLOWERS[usize::from(prop.yaw % 4)],
        PropKind::Reeds => "#a2ad6f",
        PropKind::Tent => "#cec4ad",
        PropKind::Crate => "#bca276",
        PropKind::CropRow => "#cebf6f",
    }
}

fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02x}{green:02x}{blue:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{MAX_PLAYERS_PER_ZONE, greyhaven_vale::SPAWN_GRID};
    use mmorpg_scenery::visual_kind;
    use serde_json::Value;

    use crate::host::hosted_definition;

    #[test]
    fn scenery_comes_from_the_hosted_content_revision() {
        let definition = hosted_definition();
        assert_eq!(hosted_scenery().content_revision, definition.revision());
        assert_eq!(
            hosted_export().content_revision,
            definition.revision().to_string()
        );
    }

    #[test]
    fn every_structure_collider_is_one_block_on_its_exact_collider() {
        let definition = hosted_definition();
        let export = hosted_export();
        let mut visualised: Vec<u32> = export
            .props
            .iter()
            .filter_map(|prop| prop.collider_id)
            .collect();
        visualised.sort_unstable();
        let structures: Vec<u32> = definition
            .colliders()
            .iter()
            .filter(|collider| visual_kind(collider).is_some())
            .map(|collider| collider.id)
            .collect();
        assert_eq!(visualised, structures, "one block per structure collider");
        assert!(
            visualised
                .iter()
                .all(|id| *id != greyhaven_vale::ids::GROUND
                    && !greyhaven_vale::ids::BOUNDARY_WALLS.contains(id)),
            "the ground is terrain and the boundary walls are invisible"
        );
        for prop in &export.props {
            let Some(id) = prop.collider_id else {
                continue;
            };
            let collider = definition
                .colliders()
                .iter()
                .find(|collider| collider.id == id)
                .unwrap();
            assert_eq!(prop.position, collider.position, "collider {id}");
            assert_eq!(prop.half_extents, collider.half_extents, "collider {id}");
            assert_eq!(prop.yaw, 0, "collider {id}");
        }
    }

    #[test]
    fn every_scenery_prop_is_one_block_standing_on_the_relief() {
        let scenery = hosted_scenery();
        let export = hosted_export();
        assert_eq!(export.props.len(), scenery.props.len());
        for (block, prop) in export.props.iter().zip(&scenery.props) {
            let [x, feet, z] = prop.position;
            assert_eq!(block.kind, "block");
            assert_eq!(block.collider_id, prop.collider);
            assert_eq!(
                block.position,
                [x, feet + relief_at(x, z) + prop.half_extents[1], z]
            );
            assert_eq!(block.yaw, prop.yaw);
            assert_eq!(block.half_extents, prop.half_extents);
            assert!(block.color.len() == 7 && block.color.starts_with('#'));
        }
        let colour = |kind: PropKind| {
            export
                .props
                .iter()
                .zip(&scenery.props)
                .find(|(_, prop)| prop.kind == kind)
                .map(|(block, _)| block.color)
                .unwrap()
        };
        assert_ne!(colour(PropKind::Keep), colour(PropKind::Inn));
        assert_ne!(colour(PropKind::Bush), colour(PropKind::Keep));
    }

    #[test]
    fn terrain_samples_the_shared_relief_and_biomes() {
        let export = hosted_export();
        let terrain = &export.terrain;
        assert_eq!((terrain.columns, terrain.rows), (101, 101));
        assert_eq!(terrain.step, TERRAIN_STEP_UNITS);
        let samples = usize::try_from(terrain.columns * terrain.rows).unwrap();
        assert_eq!(terrain.heights.len(), samples);
        assert_eq!(terrain.biomes.len(), samples);
        let biome_ids: Vec<u8> = export.biomes.iter().map(|biome| biome.id).collect();
        assert!(terrain.biomes.iter().all(|id| biome_ids.contains(id)));
        let columns = usize::try_from(terrain.columns).unwrap();
        let offset = |samples: usize| TERRAIN_STEP_UNITS * i32::try_from(samples).unwrap();
        for (index, height) in terrain.heights.iter().enumerate() {
            let x = terrain.origin_xz[0] + offset(index % columns);
            let z = terrain.origin_xz[1] + offset(index / columns);
            assert_eq!(*height, relief_at(x, z), "sample {index}");
        }
        assert!(
            terrain.heights.iter().any(|height| *height > 2_000),
            "mountains beyond the walls"
        );
        let sample = |x: i32, z: i32| {
            let column = usize::try_from((x - terrain.origin_xz[0]) / TERRAIN_STEP_UNITS).unwrap();
            let row = usize::try_from((z - terrain.origin_xz[1]) / TERRAIN_STEP_UNITS).unwrap();
            terrain.biomes[row * columns + column]
        };
        assert_eq!(
            sample(0, 2_000),
            mmorpg_scenery::Biome::Plaza.id(),
            "the hub plaza"
        );
    }

    #[test]
    fn every_spawn_slot_stands_on_flat_relief() {
        for slot in 0..MAX_PLAYERS_PER_ZONE {
            let [x, _, z] = SPAWN_GRID.feet(u16::try_from(slot).unwrap()).unwrap();
            assert_eq!(relief_at(x, z), 0, "spawn slot {slot}");
        }
    }

    #[test]
    fn export_is_versioned_deterministic_json_with_core_areas() {
        let json = hosted_scenery_json();
        assert_eq!(json, serde_json::to_string(&hosted_export()).unwrap());
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["format"], SCENERY_FORMAT);
        assert_eq!(value["version"], SCENERY_FORMAT_VERSION);
        assert_eq!(value["source"], SCENERY_SOURCE);
        assert_eq!(value["contentRevision"], "2");
        assert_eq!(value["unitsPerMetre"], 100);
        assert_eq!(value["playerHalfExtents"], serde_json::json!([30, 90, 30]));
        let names: Vec<_> = value["areas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|area| area["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "Greyhaven Outpost",
                "Wolfrun Woods",
                "Millbrook Farm",
                "Stillwater Lake",
                "Redbrand Hollow"
            ]
        );
        assert_eq!(
            value["areas"][0]["maxXz"],
            serde_json::json!([3_499, 5_299]),
            "inclusive core bounds"
        );
        assert_eq!(value["props"][0]["kind"], "block");
        assert_eq!(value["biomes"][6]["name"], "plaza");
        assert_eq!(value["biomes"][6]["color"], "#96886e");
        assert_eq!(
            value["water"],
            serde_json::json!([
                { "centerXz": [5_500, -5_500], "radiiXz": [2_200, 1_600], "surfaceY": 15 }
            ])
        );
    }

    /// The local zone host is gameplay; scenery must never feed back into it.
    #[test]
    fn the_local_zone_host_never_reads_presentation_scenery() {
        let host = include_str!("host.rs");
        assert!(!host.contains("mmorpg_scenery") && !host.contains("crate::scenery"));
    }
}
