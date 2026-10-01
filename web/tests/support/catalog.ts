/** A catalog export shaped like the WASM `catalog()` output, for decoder and UI tests. */
export function catalogJson(revision = "3"): Record<string, unknown> {
  return {
    format: "mmorpg.catalog",
    version: 1,
    contentRevision: revision,
    contentFingerprint: "3cbc808bbe89b29c",
    creatureTemplates: [
      { id: 1, name: "Timber Wolf", family: "wolf", behaviour: "aggressive", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [40, 45, 40] },
      { id: 2, name: "Young Boar", family: "boar", behaviour: "neutral", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [45, 45, 45] },
    ],
    npcs: [
      { id: 5, name: "Brother Aldous", role: "spirit_healer", level: 10 },
      { id: 6, name: "Greyhaven Guard", role: "guard", level: 10 },
    ],
    areas: [{ id: 1, name: "Greyhaven Outpost" }],
  };
}
