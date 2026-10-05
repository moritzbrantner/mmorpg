/** A catalog export shaped like the WASM `catalog()` output, for decoder and UI tests. */
export function catalogJson(revision = "3"): Record<string, unknown> {
  return {
    format: "mmorpg.catalog",
    version: 5,
    itemCatalogRevision: "2",
    items: [
      { id: 1, name: "Torn Fur", maxStack: 20, slot: null, stats: { stamina: 0, strength: 0, agility: 0, intellect: 0 }, sellPrice: 1 },
      { id: 2, name: "Worn Dagger", maxStack: 1, slot: "mainHand", stats: { stamina: 0, strength: 2, agility: 2, intellect: 0 }, sellPrice: 2 },
    ],
    contentRevision: revision,
    contentFingerprint: "3cbc808bbe89b29c",
    creatureTemplates: [
      { id: 1, name: "Timber Wolf", family: "wolf", behaviour: "aggressive", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [40, 45, 40] },
      { id: 2, name: "Young Boar", family: "boar", behaviour: "neutral", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [45, 45, 45] },
    ],
    npcs: [
      { id: 3, name: "Innkeeper Bram Tolliver", role: "vendor", level: 5 },
      { id: 5, name: "Brother Aldous", role: "spirit_healer", level: 10 },
      { id: 6, name: "Greyhaven Guard", role: "guard", level: 10 },
    ],
    areas: [{ id: 1, name: "Greyhaven Outpost" }],
    classes: [
      { id: 0, name: "warden", resource: "rage" },
      { id: 1, name: "ranger", resource: "focus" },
      { id: 2, name: "arcanist", resource: "mana" },
    ],
    vendorCatalogRevision: "1",
    vendors: [{ npc: 3, offers: [{ item: 2, price: 9 }] }],
    abilityCatalogRevision: "1",
    abilities: [
      { id: 1, name: "Heroic Strike", user: "warden", level: 1, cost: 15, castTicks: 0, channel: false, cooldown: 0, aura: null },
      { id: 2, name: "Shield Bash", user: "warden", level: 2, cost: 10, castTicks: 0, channel: false, cooldown: 360, aura: 6 },
      { id: 9, name: "Firebolt", user: "arcanist", level: 1, cost: 25, castTicks: 60, channel: false, cooldown: 0, aura: null },
      { id: 13, name: "Muck Bolt", user: "creature", level: 1, cost: 0, castTicks: 45, channel: false, cooldown: 240, aura: null },
    ],
  };
}
