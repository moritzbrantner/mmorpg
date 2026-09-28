//! Versioned content catalog for clients: what projections refer to by ID.
//!
//! Player projections name creatures by template ID and NPCs by NPC ID, and
//! clients read area names from core content. The catalog (format
//! [`CATALOG_FORMAT`], version [`CATALOG_FORMAT_VERSION`]) exports those
//! tables of the hosted [`ZoneContent`] as compact JSON: creature templates
//! (name, family, behaviour, levels, elite, body size), NPCs (name, role,
//! level) and areas (name). It carries the content revision and fingerprint,
//! so a client can refuse a catalog of other content. Combat numbers,
//! spawn points and AI stay on the server side.

use std::sync::OnceLock;

use mmorpg_core::ZoneContent;
use serde::Serialize;

use crate::host::hosted_content;

pub const CATALOG_FORMAT: &str = "mmorpg.catalog";
pub const CATALOG_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogExport {
    pub format: &'static str,
    pub version: u32,
    /// Decimal `u64`, so JavaScript never rounds it.
    pub content_revision: String,
    /// The content fingerprint as 16 lower-case hex digits.
    pub content_fingerprint: String,
    pub creature_templates: Vec<CreatureTemplateExport>,
    pub npcs: Vec<NpcExport>,
    pub areas: Vec<AreaNameExport>,
}

/// `[id, name, family, behaviour, minLevel, maxLevel, elite, halfExtents]`
/// in object form.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatureTemplateExport {
    pub id: u16,
    pub name: String,
    pub family: &'static str,
    pub behaviour: &'static str,
    pub min_level: u8,
    pub max_level: u8,
    pub elite: bool,
    /// Collision box in units; presentation sizes placeholder bodies by it.
    pub half_extents: [i32; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct NpcExport {
    pub id: u32,
    pub name: String,
    pub role: &'static str,
    pub level: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct AreaNameExport {
    pub id: u16,
    pub name: String,
}

/// The catalog of `content`, every table ordered by ID.
#[must_use]
pub fn catalog(content: &ZoneContent) -> CatalogExport {
    CatalogExport {
        format: CATALOG_FORMAT,
        version: CATALOG_FORMAT_VERSION,
        content_revision: content.revision().to_string(),
        content_fingerprint: format!("{:016x}", content.fingerprint()),
        creature_templates: content
            .creature_templates()
            .iter()
            .map(|template| CreatureTemplateExport {
                id: template.id.get(),
                name: template.name.clone(),
                family: template.family.name(),
                behaviour: template.behaviour.name(),
                min_level: template.min_level,
                max_level: template.max_level,
                elite: template.elite,
                half_extents: template.half_extents,
            })
            .collect(),
        npcs: content
            .npcs()
            .iter()
            .map(|npc| NpcExport {
                id: npc.id.get(),
                name: npc.name.clone(),
                role: npc.role.name(),
                level: npc.level,
            })
            .collect(),
        areas: content
            .areas()
            .areas()
            .iter()
            .map(|area| AreaNameExport {
                id: area.id().get(),
                name: area.name().to_owned(),
            })
            .collect(),
    }
}

/// The serialized catalog of the hosted content. Serializing plain data
/// cannot fail.
#[must_use]
pub fn hosted_catalog_json() -> &'static str {
    static JSON: OnceLock<String> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::to_string(&catalog(&hosted_content()))
            .expect("catalog export is plain serializable data")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn the_catalog_is_versioned_json_of_the_hosted_content() {
        let json = hosted_catalog_json();
        let value: Value = serde_json::from_str(json).unwrap();
        let content = hosted_content();
        assert_eq!(value["format"], CATALOG_FORMAT);
        assert_eq!(value["version"], CATALOG_FORMAT_VERSION);
        assert_eq!(value["contentRevision"], content.revision().to_string());
        assert_eq!(
            value["contentFingerprint"],
            format!("{:016x}", content.fingerprint())
        );
        assert_eq!(
            value["creatureTemplates"][0],
            json!({
                "id": 1,
                "name": "Timber Wolf",
                "family": "wolf",
                "behaviour": "aggressive",
                "minLevel": 1,
                "maxLevel": 2,
                "elite": false,
                "halfExtents": [40, 45, 40],
            })
        );
        let garrick = &value["creatureTemplates"][6];
        assert_eq!(
            (&garrick["name"], &garrick["elite"]),
            (&json!("Garrick Redbrand"), &json!(true))
        );
        assert_eq!(
            value["npcs"][4],
            json!({ "id": 5, "name": "Brother Aldous", "role": "spirit_healer", "level": 10 })
        );
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
            value["creatureTemplates"].as_array().unwrap().len(),
            content.creature_templates().len()
        );
        assert_eq!(
            value["npcs"].as_array().unwrap().len(),
            content.npcs().len()
        );
    }

    #[test]
    fn the_catalog_is_deterministic_and_carries_no_server_numbers() {
        assert_eq!(
            hosted_catalog_json(),
            serde_json::to_string(&catalog(&hosted_content())).unwrap()
        );
        let json = hosted_catalog_json();
        for secret in ["damage", "health", "swing", "respawn", "position", "wander"] {
            assert!(!json.contains(secret), "the catalog exposes {secret}");
        }
    }
}
