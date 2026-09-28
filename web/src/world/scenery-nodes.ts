import type { IndexedMeshGeometry, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { Color, Prop, Scenery } from "./scenery";

/**
 * Static renderer nodes for a decoded scenery, built once per content
 * revision: one terrain mesh per biome (the pinned renderer has no vertex
 * colours), props merged into one mesh per colour, and a disc per water body.
 * The node count stays small however many props the scenery carries.
 */
type Point = [number, number, number];

const YAW_STEPS = 65_536;
const WATER_COLOR: Color = "#3f6f86";
const WATER_THICKNESS_METRES = 0.04;

function resourcePrefix(scenery: Scenery): string {
  return `scenery:${scenery.source}:${scenery.contentRevision}`;
}

function terrainNodes(scenery: Scenery): RendererSceneNode[] {
  const { terrain, unitsPerMetre } = scenery;
  const metres = (units: number) => units / unitsPerMetre;
  const meshes = new Map<number, { positions: Point[]; indices: number[]; local: Map<number, number> }>();
  const vertex = (mesh: { positions: Point[]; local: Map<number, number> }, sample: number): number => {
    const existing = mesh.local.get(sample);
    if (existing !== undefined) {
      return existing;
    }
    const column = sample % terrain.columns;
    const row = Math.floor(sample / terrain.columns);
    const index = mesh.positions.length;
    mesh.positions.push([
      metres(terrain.originXz[0] + column * terrain.step),
      metres(terrain.heights[sample] ?? 0),
      metres(terrain.originXz[1] + row * terrain.step),
    ]);
    mesh.local.set(sample, index);
    return index;
  };
  for (let row = 0; row + 1 < terrain.rows; row += 1) {
    for (let column = 0; column + 1 < terrain.columns; column += 1) {
      const s00 = row * terrain.columns + column;
      // A cell takes the biome of its minimum corner.
      const biome = terrain.biomes[s00] ?? 0;
      let mesh = meshes.get(biome);
      if (!mesh) {
        mesh = { positions: [], indices: [], local: new Map() };
        meshes.set(biome, mesh);
      }
      const i00 = vertex(mesh, s00);
      const i10 = vertex(mesh, s00 + 1);
      const i01 = vertex(mesh, s00 + terrain.columns);
      const i11 = vertex(mesh, s00 + terrain.columns + 1);
      // Counter-clockwise seen from above, so faces point up.
      mesh.indices.push(i00, i01, i10, i10, i01, i11);
    }
  }
  const colors = new Map(scenery.biomes.map((biome) => [biome.id, biome.color]));
  return [...meshes.entries()]
    .sort(([left], [right]) => left - right)
    .map(([biome, mesh]) => ({
      id: `terrain-${biome}`,
      geometry: {
        kind: "mesh",
        resourceKey: `${resourcePrefix(scenery)}:terrain:${biome}`,
        positions: mesh.positions,
        indices: mesh.indices,
      },
      color: colors.get(biome) ?? "#808080",
      transform: { translation: [0, 0, 0] },
    }));
}

/** Six outward faces: normal axis, then two tangent axes whose cross product is the normal. */
const BOX_FACES: readonly (readonly [Point, Point, Point])[] = [
  [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
  [[-1, 0, 0], [0, 0, 1], [0, 1, 0]],
  [[0, 1, 0], [0, 0, 1], [1, 0, 0]],
  [[0, -1, 0], [1, 0, 0], [0, 0, 1]],
  [[0, 0, 1], [1, 0, 0], [0, 1, 0]],
  [[0, 0, -1], [0, 1, 0], [1, 0, 0]],
];

/** Appends one yaw-rotated box in metres: 24 vertices with per-face normals. */
export function appendBox(
  mesh: { positions: Point[]; normals: Point[]; indices: number[] },
  centre: Point,
  halfExtents: Point,
  yaw: number,
): void {
  const radians = (yaw / YAW_STEPS) * 2 * Math.PI;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  // Yaw turns local +Z toward +X: world = (x cos + z sin, y, -x sin + z cos).
  const rotate = ([x, y, z]: Point): Point => [x * cos + z * sin, y, -x * sin + z * cos];
  for (const [normal, u, v] of BOX_FACES) {
    const base = mesh.positions.length;
    for (const [su, sv] of [[-1, -1], [1, -1], [1, 1], [-1, 1]] as const) {
      const local: Point = [0, 1, 2].map((axis) =>
        (normal[axis]! + u[axis]! * su + v[axis]! * sv) * halfExtents[axis]!) as Point;
      const world = rotate(local);
      mesh.positions.push([centre[0] + world[0], centre[1] + world[1], centre[2] + world[2]]);
      mesh.normals.push(rotate(normal));
    }
    mesh.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
  }
}

function propNodes(scenery: Scenery): RendererSceneNode[] {
  const metres = (units: number) => units / scenery.unitsPerMetre;
  const byColor = new Map<Color, { positions: Point[]; normals: Point[]; indices: number[] }>();
  const add = (prop: Prop) => {
    switch (prop.kind) {
      case "block": {
        let mesh = byColor.get(prop.color);
        if (!mesh) {
          mesh = { positions: [], normals: [], indices: [] };
          byColor.set(prop.color, mesh);
        }
        appendBox(
          mesh,
          [metres(prop.position[0]), metres(prop.position[1]), metres(prop.position[2])],
          [metres(prop.halfExtents[0]), metres(prop.halfExtents[1]), metres(prop.halfExtents[2])],
          prop.yaw,
        );
        return;
      }
      default: {
        const unsupported: never = prop.kind;
        throw new Error(`Unsupported prop kind ${String(unsupported)}.`);
      }
    }
  };
  scenery.props.forEach(add);
  return [...byColor.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([color, mesh]) => {
      const geometry: IndexedMeshGeometry = {
        kind: "mesh",
        resourceKey: `${resourcePrefix(scenery)}:props:${color}`,
        positions: mesh.positions,
        normals: mesh.normals,
        indices: mesh.indices,
      };
      return { id: `props-${color.slice(1)}`, geometry, color, transform: { translation: [0, 0, 0] } };
    });
}

function waterNodes(scenery: Scenery): RendererSceneNode[] {
  const metres = (units: number) => units / scenery.unitsPerMetre;
  return scenery.water.map((water, index) => ({
    id: `water-${index}`,
    geometry: { kind: "cylinder", radius: 1, height: WATER_THICKNESS_METRES },
    color: WATER_COLOR,
    opacity: 0.82,
    transform: {
      translation: [metres(water.centerXz[0]), metres(water.surfaceY) - WATER_THICKNESS_METRES / 2, metres(water.centerXz[1])],
      scale: [metres(water.radiiXz[0]), 1, metres(water.radiiXz[1])],
    },
  }));
}

export function buildSceneryNodes(scenery: Scenery): RendererSceneNode[] {
  return [...terrainNodes(scenery), ...propNodes(scenery), ...waterNodes(scenery)];
}
