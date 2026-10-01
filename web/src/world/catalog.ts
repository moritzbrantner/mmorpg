/**
 * The content catalog from the WASM `catalog()` export (format
 * `mmorpg.catalog` v2): names and presentation facts for the IDs that
 * projections carry. Creature templates by template ID, NPCs by NPC ID,
 * areas by area ID. Combat numbers stay on the server.
 */
export type CreatureFamily = "wolf" | "boar" | "vermin" | "marauder" | "mirefin" | "redbrand";
export type CreatureBehaviour = "aggressive" | "neutral";
export type NpcRole = "quest_giver" | "vendor" | "spirit_healer" | "guard";

export type CreatureTemplate = {
  id: number;
  name: string;
  family: CreatureFamily;
  behaviour: CreatureBehaviour;
  minLevel: number;
  maxLevel: number;
  elite: boolean;
  /** Collision box in units. */
  halfExtents: readonly [number, number, number];
};

export type NpcRecord = { id: number; name: string; role: NpcRole; level: number };

export type ItemRecord = { id: number; name: string; maxStack: number };

export type ContentCatalog = {
  itemCatalogRevision: bigint;
  items: ReadonlyMap<number, ItemRecord>;
  contentRevision: bigint;
  /** 16 lower-case hex digits. */
  contentFingerprint: string;
  creatureTemplates: ReadonlyMap<number, CreatureTemplate>;
  npcs: ReadonlyMap<number, NpcRecord>;
  areas: ReadonlyMap<number, string>;
};

const FORMAT = "mmorpg.catalog";
const VERSION = 2;
const FAMILIES: readonly CreatureFamily[] = ["wolf", "boar", "vermin", "marauder", "mirefin", "redbrand"];
const BEHAVIOURS: readonly CreatureBehaviour[] = ["aggressive", "neutral"];
const ROLES: readonly NpcRole[] = ["quest_giver", "vendor", "spirit_healer", "guard"];

type Json = Record<string, unknown>;

function fail(message: string): never {
  throw new Error(`Invalid catalog: ${message}.`);
}

function object(value: unknown, keys: readonly string[], name: string): Json {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${name} must be an object`);
  }
  const record = value as Json;
  if (Object.keys(record).length !== keys.length || keys.some((key) => !Object.hasOwn(record, key))) {
    fail(`${name} must have exactly ${keys.join(", ")}`);
  }
  return record;
}

function int(value: unknown, name: string, min: number, max: number): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < min || value > max) {
    fail(`${name} must be an integer in [${min}, ${max}]`);
  }
  return value;
}

function list(value: unknown, name: string, maximum: number): readonly unknown[] {
  if (!Array.isArray(value) || value.length > maximum) {
    fail(`${name} must be an array of at most ${maximum} entries`);
  }
  return value;
}

function text(value: unknown, name: string): string {
  if (typeof value !== "string" || value.trim().length === 0 || value.length > 64) {
    fail(`${name} must be non-empty text`);
  }
  return value;
}

function oneOf<T extends string>(value: unknown, options: readonly T[], name: string): T {
  const match = options.find((option) => option === value);
  if (match === undefined) {
    fail(`${name} must be one of ${options.join(", ")}`);
  }
  return match;
}

/** Indexes records by ID; duplicate IDs fail closed. */
function byId<T extends { id: number }>(records: readonly T[], name: string): ReadonlyMap<number, T> {
  const map = new Map(records.map((record) => [record.id, record]));
  if (map.size !== records.length) {
    fail(`${name} IDs must be unique`);
  }
  return map;
}

/** Strictly decodes the export at the WASM boundary; unknown shapes fail closed. */
export function decodeCatalog(json: string): ContentCatalog {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    fail("not JSON");
  }
  const root = object(parsed, [
    "format", "version", "contentRevision", "contentFingerprint", "creatureTemplates", "npcs", "areas", "itemCatalogRevision", "items",
  ], "export");
  if (root.format !== FORMAT || root.version !== VERSION) {
    fail(`unsupported format ${String(root.format)} v${String(root.version)}`);
  }
  if (typeof root.contentRevision !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(root.contentRevision)) {
    fail("content revision must be a canonical decimal u64");
  }
  const contentRevision = BigInt(root.contentRevision);
  if (contentRevision > 0xffff_ffff_ffff_ffffn) {
    fail("content revision exceeds u64");
  }
  if (typeof root.contentFingerprint !== "string" || !/^[0-9a-f]{16}$/.test(root.contentFingerprint)) {
    fail("content fingerprint must be 16 lower-case hex digits");
  }
  const templates = list(root.creatureTemplates, "creature templates", 256).map((value, index) => {
    const name = `creature template ${index}`;
    const record = object(value, [
      "id", "name", "family", "behaviour", "minLevel", "maxLevel", "elite", "halfExtents",
    ], name);
    const minLevel = int(record.minLevel, `${name} minimum level`, 1, 255);
    const maxLevel = int(record.maxLevel, `${name} maximum level`, minLevel, 255);
    const extents = list(record.halfExtents, `${name} half extents`, 3);
    if (extents.length !== 3 || typeof record.elite !== "boolean") {
      fail(`${name} needs three half extents and an elite flag`);
    }
    const [x = 0, y = 0, z = 0] = extents.map((extent, axis) => int(extent, `${name} half extent ${axis}`, 1, 10_000));
    return {
      id: int(record.id, `${name} id`, 0, 0xffff),
      name: text(record.name, `${name} name`),
      family: oneOf(record.family, FAMILIES, `${name} family`),
      behaviour: oneOf(record.behaviour, BEHAVIOURS, `${name} behaviour`),
      minLevel,
      maxLevel,
      elite: record.elite,
      halfExtents: [x, y, z] as const,
    };
  });
  const npcs = list(root.npcs, "npcs", 256).map((value, index) => {
    const name = `npc ${index}`;
    const record = object(value, ["id", "name", "role", "level"], name);
    return {
      id: int(record.id, `${name} id`, 0, 0xffff),
      name: text(record.name, `${name} name`),
      role: oneOf(record.role, ROLES, `${name} role`),
      level: int(record.level, `${name} level`, 1, 255),
    };
  });
  const areas = list(root.areas, "areas", 64).map((value, index) => {
    const record = object(value, ["id", "name"], `area ${index}`);
    return { id: int(record.id, `area ${index} id`, 0, 0xffff), name: text(record.name, `area ${index} name`) };
  });
  if (root.itemCatalogRevision !== "1") fail("unsupported item catalog revision");
  const items = list(root.items, "items", 256).map((value, index) => {
    const record = object(value, ["id", "name", "maxStack"], `item ${index}`);
    return {
      id: int(record.id, `item ${index} id`, 1, 0xffff),
      name: text(record.name, `item ${index} name`),
      maxStack: int(record.maxStack, `item ${index} stack limit`, 1, 0xffff),
    };
  });
  return {
    itemCatalogRevision: 1n,
    items: byId(items, "item"),
    contentRevision,
    contentFingerprint: root.contentFingerprint,
    creatureTemplates: byId(templates, "creature template"),
    npcs: byId(npcs, "npc"),
    areas: new Map([...byId(areas, "area")].map(([id, area]) => [id, area.name])),
  };
}
