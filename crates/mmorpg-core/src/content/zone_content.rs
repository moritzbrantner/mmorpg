//! The complete immutable content of one zone and its identity.
//!
//! A zone's content is its physical definition plus its named areas, creature
//! templates, creature spawns, NPCs and graveyard. [`ZoneContent::new`]
//! validates the tables once and computes a fingerprint over a canonical byte
//! encoding of all of them. Canonical snapshots record the revision and
//! fingerprint instead of embedding the content, and recovery refuses content
//! whose identity differs (the content-addressed checkpoint rule).

use super::units::{
    CreatureSpawn, CreatureTemplate, MAX_CREATURE_HALF_EXTENT_UNITS, MAX_CREATURE_SPAWNS,
    MAX_CREATURE_TEMPLATES, MAX_NPCS, MAX_UNIT_NAME_BYTES, MAX_WANDER_RADIUS_UNITS, Npc,
};
use super::{MAX_CONTENT_COORDINATE_UNITS, ZoneDefinition};
use crate::unit::{CORPSE_TICKS, MAX_SWING_DAMAGE, MAX_UNIT_LEVEL};
use crate::{
    CreatureId, CreatureTemplateId, MAX_PLAYERS_PER_ZONE, NpcId, PLAYER_HALF_EXTENTS_UNITS,
    ZoneAreas, ZoneError,
};

/// Immutable, validated zone content, shared between zones through `Arc`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneContent {
    definition: ZoneDefinition,
    areas: ZoneAreas,
    templates: Vec<CreatureTemplate>,
    spawns: Vec<CreatureSpawn>,
    npcs: Vec<Npc>,
    graveyard: [i32; 2],
    fingerprint: u64,
    rng_seed: u64,
}

/// A body as `(centre, half extents)` in units.
type BodyBox = ([i32; 3], [i32; 3]);

impl ZoneContent {
    /// Validates and orders the unit tables:
    ///
    /// - IDs are unique per table; tables are bounded and sorted by ID;
    /// - templates have names, level ranges within `1..=MAX_UNIT_LEVEL`,
    ///   positive health and damage whose maximum-level values fit their
    ///   types, a positive swing time, a respawn time of at least the corpse
    ///   duration, and positive bounded half extents;
    /// - spawns name a known template and a wander radius in
    ///   `0..=MAX_WANDER_RADIUS_UNITS`; NPCs have names and levels;
    /// - every creature, NPC and graveyard body stands on y = 0 inside the
    ///   content coordinate range without overlapping a collider, another
    ///   unit's body or a player spawn slot (the graveyard may share room with
    ///   spawn slots and other released spirits).
    pub fn new(
        definition: ZoneDefinition,
        areas: ZoneAreas,
        mut templates: Vec<CreatureTemplate>,
        mut spawns: Vec<CreatureSpawn>,
        mut npcs: Vec<Npc>,
        graveyard: [i32; 2],
    ) -> Result<Self, ZoneError> {
        if templates.len() > MAX_CREATURE_TEMPLATES
            || spawns.len() > MAX_CREATURE_SPAWNS
            || npcs.len() > MAX_NPCS
        {
            return Err(ZoneError::new("zone unit content capacity reached"));
        }
        templates.sort_by_key(|template| template.id);
        spawns.sort_by_key(|spawn| spawn.id);
        npcs.sort_by_key(|npc| npc.id);
        if templates.windows(2).any(|pair| pair[0].id == pair[1].id)
            || spawns.windows(2).any(|pair| pair[0].id == pair[1].id)
            || npcs.windows(2).any(|pair| pair[0].id == pair[1].id)
        {
            return Err(ZoneError::new("duplicate unit content id"));
        }
        for template in &templates {
            validate_template(template)?;
        }

        let fixed_boxes = spawn_slot_boxes(&definition)?;
        let mut unit_boxes: Vec<BodyBox> = Vec::new();
        for spawn in &spawns {
            let template = templates
                .binary_search_by_key(&spawn.template, |template| template.id)
                .map(|index| &templates[index])
                .map_err(|_| ZoneError::new("creature spawn names an unknown template"))?;
            if !(0..=MAX_WANDER_RADIUS_UNITS).contains(&spawn.wander_radius) {
                return Err(ZoneError::new("creature wander radius is out of range"));
            }
            let body = standing_body(spawn.position, template.half_extents);
            validate_unit_body(&definition, body, &unit_boxes, &fixed_boxes)?;
            unit_boxes.push(body);
        }
        for npc in &npcs {
            validate_name(&npc.name)?;
            // Projections carry an NPC's ID as its `u16` appearance.
            if u16::try_from(npc.id.get()).is_err() {
                return Err(ZoneError::new("npc id must fit u16"));
            }
            if !(1..=MAX_UNIT_LEVEL).contains(&npc.level) {
                return Err(ZoneError::new("npc level is out of range"));
            }
            let body = standing_body(npc.position, PLAYER_HALF_EXTENTS_UNITS);
            validate_unit_body(&definition, body, &unit_boxes, &fixed_boxes)?;
            unit_boxes.push(body);
        }
        // Released spirits may stand where players spawn or where other
        // spirits stand, but never inside geometry or an NPC.
        let graveyard_body = standing_body(graveyard, PLAYER_HALF_EXTENTS_UNITS);
        let npc_boxes = &unit_boxes[spawns.len()..];
        validate_unit_body(&definition, graveyard_body, npc_boxes, &[])?;

        let mut content = Self {
            definition,
            areas,
            templates,
            spawns,
            npcs,
            graveyard,
            fingerprint: 0,
            rng_seed: 0,
        };
        content.rng_seed = content.compute_simulation_fingerprint();
        content.fingerprint = content.compute_fingerprint();
        Ok(content)
    }

    /// Physical content only: no areas, creatures or NPCs, and the graveyard
    /// at spawn slot 0, which the definition already proves clear.
    #[must_use]
    pub fn from_definition(definition: ZoneDefinition) -> Self {
        let feet = definition.spawn_grid().feet(0).unwrap_or([0; 3]);
        let mut content = Self {
            definition,
            areas: ZoneAreas::default(),
            templates: Vec::new(),
            spawns: Vec::new(),
            npcs: Vec::new(),
            graveyard: [feet[0], feet[2]],
            fingerprint: 0,
            rng_seed: 0,
        };
        content.rng_seed = content.compute_simulation_fingerprint();
        content.fingerprint = content.compute_fingerprint();
        content
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.definition.revision()
    }

    /// Stable FNV-1a 64 hash of the canonical encoding of every table.
    #[must_use]
    pub const fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    #[must_use]
    pub const fn definition(&self) -> &ZoneDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn areas(&self) -> &ZoneAreas {
        &self.areas
    }

    /// Ordered by ID.
    #[must_use]
    pub fn creature_templates(&self) -> &[CreatureTemplate] {
        &self.templates
    }

    #[must_use]
    pub fn creature_template(&self, id: CreatureTemplateId) -> Option<&CreatureTemplate> {
        self.templates
            .binary_search_by_key(&id, |template| template.id)
            .ok()
            .map(|index| &self.templates[index])
    }

    /// Ordered by ID.
    #[must_use]
    pub fn creature_spawns(&self) -> &[CreatureSpawn] {
        &self.spawns
    }

    #[must_use]
    pub fn creature_spawn(&self, id: CreatureId) -> Option<&CreatureSpawn> {
        self.spawns
            .binary_search_by_key(&id, |spawn| spawn.id)
            .ok()
            .map(|index| &self.spawns[index])
    }

    /// Ordered by ID.
    #[must_use]
    pub fn npcs(&self) -> &[Npc] {
        &self.npcs
    }

    #[must_use]
    pub fn npc(&self, id: NpcId) -> Option<&Npc> {
        self.npcs
            .binary_search_by_key(&id, |npc| npc.id)
            .ok()
            .map(|index| &self.npcs[index])
    }

    /// Feet position on y = 0 where released spirits return.
    #[must_use]
    pub const fn graveyard(&self) -> [i32; 2] {
        self.graveyard
    }

    /// An authored RNG seed is part of recovery identity, while economy-only
    /// content changes may intentionally preserve the existing random stream.
    #[must_use]
    pub fn with_rng_seed(mut self, seed: u64) -> Self {
        self.rng_seed = seed;
        self.fingerprint = self.compute_fingerprint();
        self
    }

    #[must_use]
    pub const fn rng_seed(&self) -> u64 {
        self.rng_seed
    }

    fn compute_fingerprint(&self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v2");
        hash.u64(self.compute_simulation_fingerprint());
        hash.u64(self.rng_seed);
        hash.u64(crate::ITEM_CATALOG_REVISION);
        hash.len(crate::ITEM_CATALOG.len());
        for item in crate::ITEM_CATALOG {
            hash.u16(item.id.get());
            hash.text(item.name);
            hash.u16(item.max_stack);
        }
        for slot in crate::Inventory::starter().slots() {
            hash.u16(slot.map_or(0, |stack| stack.item().get()));
            hash.u16(slot.map_or(0, |stack| stack.quantity()));
        }
        hash.finish()
    }

    // Preserve the pre-inventory default seed for unchanged physical/unit content.
    fn compute_simulation_fingerprint(&self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v1");
        let definition = &self.definition;
        hash.u64(definition.revision());
        hash.i32s(&definition.gravity());
        let grid = definition.spawn_grid();
        hash.i32s(&grid.origin);
        hash.u16(grid.columns);
        hash.i32(grid.spacing);
        hash.len(definition.colliders().len());
        for collider in definition.colliders() {
            hash.u32(collider.id);
            hash.i32s(&collider.position);
            hash.i32s(&collider.half_extents);
        }
        hash.len(self.areas.areas().len());
        for area in self.areas.areas() {
            hash.u16(area.id().get());
            hash.text(area.name());
            hash.i32s(&area.min_xz());
            hash.i32s(&area.max_xz());
        }
        hash.len(self.templates.len());
        for template in &self.templates {
            hash.u16(template.id.get());
            hash.text(&template.name);
            hash.u8(family_code(template.family));
            hash.u8(match template.behaviour {
                super::units::CreatureBehaviour::Aggressive => 1,
                super::units::CreatureBehaviour::Neutral => 2,
            });
            hash.u8(template.min_level);
            hash.u8(template.max_level);
            hash.u8(u8::from(template.elite));
            hash.u32(template.health);
            hash.u32(template.health_per_level);
            hash.u16(template.damage[0]);
            hash.u16(template.damage[1]);
            hash.u16(template.damage_per_level);
            hash.u16(template.swing_ticks);
            hash.u32(template.respawn_ticks);
            hash.i32s(&template.half_extents);
        }
        hash.len(self.spawns.len());
        for spawn in &self.spawns {
            hash.u32(spawn.id.get());
            hash.u16(spawn.template.get());
            hash.i32s(&spawn.position);
            hash.u16(spawn.facing);
            hash.i32(spawn.wander_radius);
        }
        hash.len(self.npcs.len());
        for npc in &self.npcs {
            hash.u32(npc.id.get());
            hash.text(&npc.name);
            hash.u8(match npc.role {
                super::units::NpcRole::QuestGiver => 1,
                super::units::NpcRole::Vendor => 2,
                super::units::NpcRole::SpiritHealer => 3,
                super::units::NpcRole::Guard => 4,
            });
            hash.u8(npc.level);
            hash.i32s(&npc.position);
            hash.u16(npc.facing);
        }
        hash.i32s(&self.graveyard);
        hash.finish()
    }
}

const fn family_code(family: super::units::CreatureFamily) -> u8 {
    use super::units::CreatureFamily;
    match family {
        CreatureFamily::Wolf => 1,
        CreatureFamily::Boar => 2,
        CreatureFamily::Vermin => 3,
        CreatureFamily::Marauder => 4,
        CreatureFamily::Mirefin => 5,
        CreatureFamily::Redbrand => 6,
    }
}

fn validate_name(name: &str) -> Result<(), ZoneError> {
    if name.trim().is_empty() || name.len() > MAX_UNIT_NAME_BYTES {
        return Err(ZoneError::new("unit name must be 1..=64 bytes of text"));
    }
    Ok(())
}

fn validate_template(template: &CreatureTemplate) -> Result<(), ZoneError> {
    validate_name(&template.name)?;
    if template.min_level == 0
        || template.min_level > template.max_level
        || template.max_level > MAX_UNIT_LEVEL
    {
        return Err(ZoneError::new("creature template levels are out of range"));
    }
    let levels = u32::from(template.max_level - template.min_level);
    let top_health = template
        .health_per_level
        .checked_mul(levels)
        .and_then(|bonus| bonus.checked_add(template.health));
    if template.health == 0 || top_health.is_none() {
        return Err(ZoneError::new("creature template health is out of range"));
    }
    let top_damage = u16::try_from(levels)
        .ok()
        .and_then(|levels| template.damage_per_level.checked_mul(levels))
        .and_then(|bonus| template.damage[1].checked_add(bonus));
    if template.damage[0] == 0
        || template.damage[0] > template.damage[1]
        || top_damage.is_none_or(|damage| damage > MAX_SWING_DAMAGE)
    {
        return Err(ZoneError::new("creature template damage is out of range"));
    }
    if template.swing_ticks == 0 || template.respawn_ticks < CORPSE_TICKS {
        return Err(ZoneError::new("creature template timing is out of range"));
    }
    if template
        .half_extents
        .iter()
        .any(|&extent| !(1..=MAX_CREATURE_HALF_EXTENT_UNITS).contains(&extent))
    {
        return Err(ZoneError::new(
            "creature template extents must be positive and bounded",
        ));
    }
    Ok(())
}

/// A body whose feet rest on y = 0 at `feet`.
const fn standing_body(feet: [i32; 2], half_extents: [i32; 3]) -> BodyBox {
    ([feet[0], half_extents[1], feet[1]], half_extents)
}

/// Touching is allowed; overlapping on every axis is not.
fn overlaps(left: BodyBox, right: BodyBox) -> bool {
    (0..3).all(|axis| {
        let distance = (i64::from(left.0[axis]) - i64::from(right.0[axis])).abs();
        distance < i64::from(left.1[axis]) + i64::from(right.1[axis])
    })
}

fn spawn_slot_boxes(definition: &ZoneDefinition) -> Result<Vec<BodyBox>, ZoneError> {
    (0..MAX_PLAYERS_PER_ZONE)
        .map(|slot| {
            u16::try_from(slot)
                .ok()
                .and_then(|slot| definition.spawn_grid().feet(slot))
                .map(|feet| standing_body([feet[0], feet[2]], PLAYER_HALF_EXTENTS_UNITS))
                .ok_or_else(|| ZoneError::new("spawn grid overflows"))
        })
        .collect()
}

fn validate_unit_body(
    definition: &ZoneDefinition,
    body: BodyBox,
    units: &[BodyBox],
    spawn_slots: &[BodyBox],
) -> Result<(), ZoneError> {
    let inside = (0..3).all(|axis| {
        let (Some(min), Some(max)) = (
            body.0[axis].checked_sub(body.1[axis]),
            body.0[axis].checked_add(body.1[axis]),
        ) else {
            return false;
        };
        -MAX_CONTENT_COORDINATE_UNITS <= min && max <= MAX_CONTENT_COORDINATE_UNITS
    });
    if !inside {
        return Err(ZoneError::new(
            "unit body lies outside the content coordinate range",
        ));
    }
    if definition
        .colliders()
        .iter()
        .any(|collider| overlaps(body, (collider.position, collider.half_extents)))
    {
        return Err(ZoneError::new("unit body overlaps a static collider"));
    }
    if units.iter().any(|&other| overlaps(body, other)) {
        return Err(ZoneError::new("unit body overlaps another unit"));
    }
    if spawn_slots.iter().any(|&slot| overlaps(body, slot)) {
        return Err(ZoneError::new("unit body overlaps a player spawn slot"));
    }
    Ok(())
}

/// FNV-1a 64 over big-endian fields; lengths prefix every list and string.
struct Fnv1a(u64);

impl Fnv1a {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_be_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_be_bytes());
    }

    fn i32s(&mut self, values: &[i32]) {
        for value in values {
            self.i32(*value);
        }
    }

    fn len(&mut self, length: usize) {
        self.u64(u64::try_from(length).unwrap_or(u64::MAX));
    }

    fn text(&mut self, text: &str) {
        self.len(text.len());
        self.bytes(text.as_bytes());
    }

    const fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::super::units::{CreatureBehaviour, CreatureFamily, NpcRole};
    use super::*;
    use crate::{SpawnGrid, StaticCollider};

    fn ground() -> ZoneDefinition {
        ZoneDefinition::with_spawn_grid(
            9,
            [0, -1, 0],
            SpawnGrid {
                origin: [0, 0],
                columns: 32,
                spacing: 100,
            },
            vec![StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [10_000, 50, 10_000],
            }],
        )
        .unwrap()
    }

    fn wolf() -> CreatureTemplate {
        CreatureTemplate {
            id: CreatureTemplateId::new(1),
            name: "Timber Wolf".into(),
            family: CreatureFamily::Wolf,
            behaviour: CreatureBehaviour::Aggressive,
            min_level: 1,
            max_level: 2,
            elite: false,
            health: 42,
            health_per_level: 14,
            damage: [2, 4],
            damage_per_level: 1,
            swing_ticks: 60,
            respawn_ticks: 1_800,
            half_extents: [40, 45, 40],
        }
    }

    fn spawn(id: u32, position: [i32; 2]) -> CreatureSpawn {
        CreatureSpawn {
            id: CreatureId::new(id),
            template: CreatureTemplateId::new(1),
            position,
            facing: 0,
            wander_radius: 600,
        }
    }

    fn npc(id: u32, position: [i32; 2]) -> Npc {
        Npc {
            id: NpcId::new(id),
            name: "Guard".into(),
            role: NpcRole::Guard,
            level: 10,
            position,
            facing: 0,
        }
    }

    fn content(
        templates: Vec<CreatureTemplate>,
        spawns: Vec<CreatureSpawn>,
        npcs: Vec<Npc>,
        graveyard: [i32; 2],
    ) -> Result<ZoneContent, ZoneError> {
        ZoneContent::new(
            ground(),
            ZoneAreas::default(),
            templates,
            spawns,
            npcs,
            graveyard,
        )
    }

    #[test]
    fn tables_are_ordered_and_scaled_by_level() {
        let content = content(
            vec![wolf()],
            vec![spawn(9, [0, 5_000]), spawn(3, [1_000, 5_000])],
            vec![npc(2, [-1_000, 5_000])],
            [0, -1_000],
        )
        .unwrap();
        let ids: Vec<_> = content
            .creature_spawns()
            .iter()
            .map(|spawn| spawn.id.get())
            .collect();
        assert_eq!(ids, [3, 9]);
        let template = content
            .creature_template(CreatureTemplateId::new(1))
            .unwrap();
        assert_eq!(template.max_health(1), 42);
        assert_eq!(template.max_health(2), 56);
        assert_eq!(template.max_health(9), 56, "clamped to the level range");
        assert_eq!(template.damage_at(2), [3, 5]);
        assert!(content.creature_spawn(CreatureId::new(9)).is_some());
        assert!(content.npc(NpcId::new(2)).is_some());
        assert_eq!(content.graveyard(), [0, -1_000]);
    }

    #[test]
    fn invalid_tables_fail_closed() {
        let valid = || (vec![wolf()], vec![spawn(1, [0, 5_000])], vec![]);
        let mut cases: Vec<(&str, Result<ZoneContent, ZoneError>)> = Vec::new();
        let (templates, _, _) = valid();
        cases.push((
            "unknown template",
            content(
                templates,
                vec![CreatureSpawn {
                    template: CreatureTemplateId::new(7),
                    ..spawn(1, [0, 5_000])
                }],
                vec![],
                [0, -1_000],
            ),
        ));
        for mutate in [
            (|template: &mut CreatureTemplate| template.min_level = 0) as fn(&mut CreatureTemplate),
            |template| template.max_level = 0,
            |template| template.health = 0,
            |template| template.health_per_level = u32::MAX,
            |template| template.damage = [5, 4],
            |template| template.damage = [0, 4],
            |template| template.damage = [2, MAX_SWING_DAMAGE],
            |template| template.swing_ticks = 0,
            |template| template.respawn_ticks = CORPSE_TICKS - 1,
            |template| template.half_extents = [0, 45, 40],
            |template| template.name = " ".into(),
        ] {
            let mut template = wolf();
            mutate(&mut template);
            let (_, spawns, npcs) = valid();
            cases.push((
                "template",
                content(vec![template], spawns, npcs, [0, -1_000]),
            ));
        }
        let (templates, _, _) = valid();
        cases.push((
            "overlapping spawns",
            content(
                templates.clone(),
                vec![spawn(1, [0, 5_000]), spawn(2, [50, 5_000])],
                vec![],
                [0, -1_000],
            ),
        ));
        cases.push((
            "spawn on a spawn slot",
            content(
                templates.clone(),
                vec![spawn(1, [0, 0])],
                vec![],
                [0, -1_000],
            ),
        ));
        cases.push((
            "spawn in the ground",
            content(
                templates.clone(),
                vec![spawn(1, [0, 5_000])],
                vec![],
                [0, -1_000],
            )
            .and_then(|_| {
                ZoneContent::new(
                    ZoneDefinition::new(
                        9,
                        [0, -1, 0],
                        vec![StaticCollider {
                            id: 1,
                            position: [0, 50, 5_000],
                            half_extents: [100, 50, 100],
                        }],
                    )?,
                    ZoneAreas::default(),
                    templates.clone(),
                    vec![spawn(1, [0, 5_000])],
                    vec![],
                    [0, -1_000],
                )
            }),
        ));
        cases.push((
            "duplicate npc",
            content(
                templates.clone(),
                vec![],
                vec![npc(1, [0, 5_000]), npc(1, [1_000, 5_000])],
                [0, -1_000],
            ),
        ));
        cases.push((
            "npc on a creature",
            content(
                templates.clone(),
                vec![spawn(1, [0, 5_000])],
                vec![npc(1, [30, 5_000])],
                [0, -1_000],
            ),
        ));
        cases.push((
            "graveyard in an npc",
            content(
                templates.clone(),
                vec![],
                vec![npc(1, [0, 5_000])],
                [0, 5_000],
            ),
        ));
        cases.push((
            "outside the content range",
            content(
                templates.clone(),
                vec![spawn(1, [MAX_CONTENT_COORDINATE_UNITS, 5_000])],
                vec![],
                [0, -1_000],
            ),
        ));
        cases.push((
            "wander radius",
            content(
                templates,
                vec![CreatureSpawn {
                    wander_radius: MAX_WANDER_RADIUS_UNITS + 1,
                    ..spawn(1, [0, 5_000])
                }],
                vec![],
                [0, -1_000],
            ),
        ));
        for (name, result) in cases {
            assert!(result.is_err(), "{name} must be rejected");
        }
    }

    #[test]
    fn the_fingerprint_covers_every_table() {
        let base = content(
            vec![wolf()],
            vec![spawn(1, [0, 5_000])],
            vec![npc(1, [1_000, 5_000])],
            [0, -1_000],
        )
        .unwrap();
        let same = content(
            vec![wolf()],
            vec![spawn(1, [0, 5_000])],
            vec![npc(1, [1_000, 5_000])],
            [0, -1_000],
        )
        .unwrap();
        assert_eq!(base.fingerprint(), same.fingerprint());
        let variants = [
            content(
                vec![CreatureTemplate {
                    swing_ticks: 61,
                    ..wolf()
                }],
                vec![spawn(1, [0, 5_000])],
                vec![npc(1, [1_000, 5_000])],
                [0, -1_000],
            ),
            content(
                vec![wolf()],
                vec![spawn(1, [0, 5_001])],
                vec![npc(1, [1_000, 5_000])],
                [0, -1_000],
            ),
            content(
                vec![wolf()],
                vec![spawn(1, [0, 5_000])],
                vec![Npc {
                    name: "Captain".into(),
                    ..npc(1, [1_000, 5_000])
                }],
                [0, -1_000],
            ),
            content(
                vec![wolf()],
                vec![spawn(1, [0, 5_000])],
                vec![npc(1, [1_000, 5_000])],
                [0, -1_001],
            ),
        ];
        for variant in variants {
            assert_ne!(variant.unwrap().fingerprint(), base.fingerprint());
        }
        let colliders_differ = ZoneContent::from_definition(ZoneDefinition::default());
        let physical = ZoneContent::from_definition(ground());
        assert_ne!(colliders_differ.fingerprint(), physical.fingerprint());
        assert_eq!(physical.graveyard(), [0, 0]);
    }
}
