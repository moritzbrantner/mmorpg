/**
 * Presentation scenery from the WASM `scenery()` export (format
 * `mmorpg.scenery` v1). The render loop consumes only the decoded `Scenery`;
 * when `mmorpg-scenery` replaces the collider blockout behind the same export,
 * nothing here or in the render loop changes. Relief and area lookups call
 * back into Rust so both clients share one owner for them.
 */
export type Color = `#${string}`;
export type XZ = readonly [number, number];
export type XYZ = readonly [number, number, number];

export type TerrainGrid = {
  /** Units at sample (0, 0); sample (column, row) lies at origin + step × (column, row). */
  originXz: XZ;
  step: number;
  columns: number;
  rows: number;
  /** Row-major (Z outer) presentation heights in units. */
  heights: readonly number[];
  /** Row-major biome IDs. */
  biomes: readonly number[];
};

export type Biome = { id: number; name: string; color: Color };

/** A prop kind the browser can draw. Later format versions add named kinds. */
export type Prop = {
  kind: "block";
  colliderId: number | null;
  position: XYZ;
  /** u16 yaw: 0 faces +Z, increasing toward +X. */
  yaw: number;
  halfExtents: XYZ;
  color: Color;
};

export type Water = { centerXz: XZ; radiiXz: XZ; surfaceY: number };
export type Area = { id: number; name: string; minXz: XZ; maxXz: XZ };

export type Scenery = {
  source: string;
  contentRevision: bigint;
  unitsPerMetre: number;
  playerHalfExtents: XYZ;
  terrain: TerrainGrid;
  biomes: readonly Biome[];
  props: readonly Prop[];
  water: readonly Water[];
  areas: readonly Area[];
};

/** What the WASM module exposes about static zone content. */
export type SceneryContent = {
  scenery(): string;
  areaAt(x: number, z: number): number | undefined;
  reliefAt(x: number, z: number): number;
};

export type SceneryProvider = {
  readonly scenery: Scenery;
  /** The named core area at `(x, z)` in units, if any. */
  areaAt(x: number, z: number): Area | null;
  /** Presentation relief at `(x, z)` in units; units are drawn raised by it. */
  reliefAt(x: number, z: number): number;
};

const FORMAT = "mmorpg.scenery";
const VERSION = 1;
const MAX_TERRAIN_SAMPLES = 1 << 20;
const MAX_PROPS = 1 << 16;
const MAX_AREAS = 64;

type Json = Record<string, unknown>;

function fail(message: string): never {
  throw new Error(`Invalid scenery: ${message}.`);
}

function object(value: unknown, keys: readonly string[], name: string): Json {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${name} must be an object`);
  }
  const record = value as Json;
  const actual = Object.keys(record);
  if (actual.length !== keys.length || keys.some((key) => !Object.hasOwn(record, key))) {
    fail(`${name} must have exactly ${keys.join(", ")}`);
  }
  return record;
}

function int(value: unknown, name: string, min = -(2 ** 31), max = 2 ** 31 - 1): number {
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

function integers(value: unknown, length: number, name: string, min?: number): number[] {
  const entries = list(value, name, length);
  if (entries.length !== length) {
    fail(`${name} must have ${length} entries`);
  }
  return entries.map((entry, index) => int(entry, `${name}[${index}]`, min));
}

function xz(value: unknown, name: string, min?: number): XZ {
  const [x = 0, z = 0] = integers(value, 2, name, min);
  return [x, z];
}

function xyz(value: unknown, name: string, min?: number): XYZ {
  const [x = 0, y = 0, z = 0] = integers(value, 3, name, min);
  return [x, y, z];
}

function text(value: unknown, name: string): string {
  if (typeof value !== "string" || value.trim().length === 0 || value.length > 64) {
    fail(`${name} must be non-empty text`);
  }
  return value;
}

function color(value: unknown, name: string): Color {
  if (typeof value !== "string" || !/^#[0-9a-f]{6}$/i.test(value)) {
    fail(`${name} must be a #rrggbb colour`);
  }
  return value as Color;
}

function decodeTerrain(value: unknown, biomeIds: ReadonlySet<number>): TerrainGrid {
  const terrain = object(value, ["originXz", "step", "columns", "rows", "heights", "biomes"], "terrain");
  const columns = int(terrain.columns, "terrain columns", 2, 4096);
  const rows = int(terrain.rows, "terrain rows", 2, 4096);
  const samples = columns * rows;
  if (samples > MAX_TERRAIN_SAMPLES) {
    fail("terrain has too many samples");
  }
  const heights = list(terrain.heights, "terrain heights", samples).map((height, index) => int(height, `height ${index}`));
  const biomes = list(terrain.biomes, "terrain biomes", samples).map((biome, index) => {
    const id = int(biome, `biome ${index}`, 0, 255);
    if (!biomeIds.has(id)) {
      fail(`terrain sample ${index} uses unknown biome ${id}`);
    }
    return id;
  });
  if (heights.length !== samples || biomes.length !== samples) {
    fail("terrain arrays must hold columns × rows samples");
  }
  return {
    originXz: xz(terrain.originXz, "terrain origin"),
    step: int(terrain.step, "terrain step", 1),
    columns,
    rows,
    heights,
    biomes,
  };
}

function decodeProp(value: unknown, index: number): Prop {
  const prop = object(value, ["kind", "colliderId", "position", "yaw", "halfExtents", "color"], `prop ${index}`);
  if (prop.kind !== "block") {
    fail(`prop ${index} has unsupported kind ${String(prop.kind)}`);
  }
  return {
    kind: "block",
    colliderId: prop.colliderId === null ? null : int(prop.colliderId, `prop ${index} collider`, 0),
    position: xyz(prop.position, `prop ${index} position`),
    yaw: int(prop.yaw, `prop ${index} yaw`, 0, 65_535),
    halfExtents: xyz(prop.halfExtents, `prop ${index} half extents`, 1),
    color: color(prop.color, `prop ${index} colour`),
  };
}

/** Strictly decodes the export at the WASM boundary; unknown shapes fail closed. */
export function decodeScenery(json: string): Scenery {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    fail("not JSON");
  }
  const root = object(parsed, [
    "format", "version", "source", "contentRevision", "unitsPerMetre", "playerHalfExtents",
    "terrain", "biomes", "props", "water", "areas",
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
  const biomes = list(root.biomes, "biomes", 256).map((value, index) => {
    const biome = object(value, ["id", "name", "color"], `biome ${index}`);
    return { id: int(biome.id, `biome ${index} id`, 0, 255), name: text(biome.name, `biome ${index} name`), color: color(biome.color, `biome ${index} colour`) };
  });
  const biomeIds = new Set(biomes.map((biome) => biome.id));
  if (biomeIds.size !== biomes.length) {
    fail("biome IDs must be unique");
  }
  const areas = list(root.areas, "areas", MAX_AREAS).map((value, index) => {
    const area = object(value, ["id", "name", "minXz", "maxXz"], `area ${index}`);
    const minXz = xz(area.minXz, `area ${index} min`);
    const maxXz = xz(area.maxXz, `area ${index} max`);
    if (minXz[0] > maxXz[0] || minXz[1] > maxXz[1]) {
      fail(`area ${index} bounds are inverted`);
    }
    return { id: int(area.id, `area ${index} id`, 0, 65_535), name: text(area.name, `area ${index} name`), minXz, maxXz };
  });
  if (new Set(areas.map((area) => area.id)).size !== areas.length) {
    fail("area IDs must be unique");
  }
  return {
    source: text(root.source, "source"),
    contentRevision,
    unitsPerMetre: int(root.unitsPerMetre, "units per metre", 1),
    playerHalfExtents: xyz(root.playerHalfExtents, "player half extents", 1),
    terrain: decodeTerrain(root.terrain, biomeIds),
    biomes,
    props: list(root.props, "props", MAX_PROPS).map(decodeProp),
    water: list(root.water, "water", 64).map((value, index) => {
      const water = object(value, ["centerXz", "radiiXz", "surfaceY"], `water ${index}`);
      return {
        centerXz: xz(water.centerXz, `water ${index} centre`),
        radiiXz: xz(water.radiiXz, `water ${index} radii`, 1),
        surfaceY: int(water.surfaceY, `water ${index} surface`),
      };
    }),
    areas,
  };
}

/** Decodes the export once and routes relief/area queries to the Rust owners. */
export function createSceneryProvider(content: SceneryContent): SceneryProvider {
  const scenery = decodeScenery(content.scenery());
  const areasById = new Map(scenery.areas.map((area) => [area.id, area]));
  return {
    scenery,
    areaAt(x, z) {
      const id = content.areaAt(Math.round(x), Math.round(z));
      if (id === undefined) {
        return null;
      }
      const area = areasById.get(id);
      if (!area) {
        throw new Error(`Area ${id} is missing from the scenery export.`);
      }
      return area;
    },
    reliefAt(x, z) {
      return content.reliefAt(Math.round(x), Math.round(z));
    },
  };
}
