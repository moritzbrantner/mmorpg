import { describe, expect, test } from "bun:test";
import type { IndexedMeshGeometry, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { createSceneryProvider, decodeScenery } from "../src/world/scenery";
import { appendBox, buildSceneryNodes } from "../src/world/scenery-nodes";
import { placeUnit, unitNodes } from "../src/world/unit-nodes";
import { characterVisualProfile } from "../src/character-visuals";

type Point = readonly [number, number, number];

function exportJson(patch: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    format: "mmorpg.scenery",
    version: 1,
    source: "test",
    contentRevision: "7",
    unitsPerMetre: 100,
    playerHalfExtents: [30, 90, 30],
    terrain: {
      originXz: [-400, -400],
      step: 400,
      columns: 3,
      rows: 3,
      heights: [0, 0, 0, 0, 20, 0, 0, 0, 0],
      biomes: [0, 0, 1, 0, 1, 1, 0, 0, 0],
    },
    biomes: [
      { id: 0, name: "wilds", color: "#4c6b3f" },
      { id: 1, name: "courtyard", color: "#7d8a63" },
    ],
    props: [
      { kind: "block", colliderId: 2, position: [0, 150, 0], yaw: 0, halfExtents: [100, 150, 50], color: "#8a6446" },
      { kind: "block", colliderId: 3, position: [500, 100, 0], yaw: 8192, halfExtents: [50, 100, 50], color: "#8a6446" },
      { kind: "block", colliderId: null, position: [0, 50, 900], yaw: 0, halfExtents: [900, 50, 30], color: "#8b8375" },
    ],
    water: [{ centerXz: [5500, -5500], radiiXz: [1200, 800], surfaceY: -20 }],
    areas: [{ id: 1, name: "Greyhaven Outpost", minXz: [-1100, -1100], maxXz: [7300, 4100] }],
    ...patch,
  };
}

const decode = (patch: Record<string, unknown> = {}) => decodeScenery(JSON.stringify(exportJson(patch)));
const mesh = (node: RendererSceneNode) => node.geometry as IndexedMeshGeometry;

function triangleNormals(geometry: IndexedMeshGeometry): Point[] {
  const normals: Point[] = [];
  for (let index = 0; index < geometry.indices.length; index += 3) {
    const [a, b, c] = [0, 1, 2].map((offset) => geometry.positions[geometry.indices[index + offset]!]!);
    const u = [b![0] - a![0], b![1] - a![1], b![2] - a![2]];
    const v = [c![0] - a![0], c![1] - a![1], c![2] - a![2]];
    normals.push([u[1]! * v[2]! - u[2]! * v[1]!, u[2]! * v[0]! - u[0]! * v[2]!, u[0]! * v[1]! - u[1]! * v[0]!]);
  }
  return normals;
}

describe("scenery export boundary", () => {
  test("decodes the versioned export", () => {
    const scenery = decode();
    expect(scenery.contentRevision).toBe(7n);
    expect(scenery.terrain.columns).toBe(3);
    expect(scenery.props.map((prop) => prop.colliderId)).toEqual([2, 3, null]);
    expect(scenery.areas[0]?.name).toBe("Greyhaven Outpost");
  });

  test("fails closed on unknown or malformed content", () => {
    const terrain = exportJson().terrain as Record<string, unknown>;
    const [prop] = exportJson().props as Record<string, unknown>[];
    for (const patch of [
      { format: "other" },
      { version: 2 },
      { contentRevision: 7 },
      { contentRevision: "07" },
      { extra: true },
      { props: [{ ...prop, kind: "tree" }] },
      { props: [{ ...prop, halfExtents: [0, 1, 1] }] },
      { props: [{ ...prop, color: "red" }] },
      { props: [{ ...prop, yaw: 65_536 }] },
      { terrain: { ...terrain, heights: [0] } },
      { terrain: { ...terrain, biomes: [0, 0, 9, 0, 1, 1, 0, 0, 0] } },
      { terrain: { ...terrain, heights: [0, 0, 0, 0, 0.5, 0, 0, 0, 0] } },
      { areas: [{ id: 1, name: "Inverted", minXz: [1, 0], maxXz: [0, 0] }] },
      { biomes: [{ id: 0, name: "a", color: "#000000" }, { id: 0, name: "b", color: "#000000" }] },
    ]) {
      expect(() => decode(patch)).toThrow("Invalid scenery");
    }
    expect(() => decodeScenery("{")).toThrow("Invalid scenery");
  });
});

describe("static scenery nodes", () => {
  test("one upward-facing terrain mesh per biome, sharing vertices within a biome", () => {
    const nodes = buildSceneryNodes(decode());
    const terrain = nodes.filter((node) => node.id.startsWith("terrain-"));
    expect(terrain.map((node) => [node.id, node.color])).toEqual([
      ["terrain-0", "#4c6b3f"],
      ["terrain-1", "#7d8a63"],
    ]);
    const cells = terrain.reduce((sum, node) => sum + mesh(node).indices.length / 6, 0);
    expect(cells).toBe(4);
    for (const node of terrain) {
      expect(triangleNormals(mesh(node)).every((normal) => normal[1] > 0)).toBe(true);
      expect(mesh(node).positions.length).toBeLessThanOrEqual(9);
    }
    expect(mesh(terrain[1]!).positions.some((position) => position[1] === 0.2)).toBe(true);
  });

  test("props merge into one mesh per colour with outward faces", () => {
    const nodes = buildSceneryNodes(decode());
    const props = nodes.filter((node) => node.id.startsWith("props-"));
    expect(props.map((node) => node.color)).toEqual(["#8a6446", "#8b8375"]);
    expect(mesh(props[0]!).positions.length).toBe(48);
    expect(mesh(props[0]!).resourceKey).toBe("scenery:test:7:props:#8a6446");
    expect(nodes.find((node) => node.id === "water-0")).toMatchObject({
      geometry: { kind: "cylinder" },
      transform: { scale: [12, 1, 8] },
    });
  });

  test("a yawed box keeps its extents and faces outward", () => {
    const box = { positions: [] as [number, number, number][], normals: [] as [number, number, number][], indices: [] as number[] };
    appendBox(box, [10, 1, -4], [2, 1, 0.5], 16_384);
    const geometry: IndexedMeshGeometry = { kind: "mesh", resourceKey: "box", ...box };
    const xs = box.positions.map((position) => position[0]);
    const zs = box.positions.map((position) => position[2]);
    // A quarter turn swaps the X and Z extents.
    expect(Math.max(...xs) - Math.min(...xs)).toBeCloseTo(1, 9);
    expect(Math.max(...zs) - Math.min(...zs)).toBeCloseTo(4, 9);
    triangleNormals(geometry).forEach((normal, triangle) => {
      const vertex = box.positions[box.indices[triangle * 3]!]!;
      const outward = (vertex[0] - 10) * normal[0] + (vertex[1] - 1) * normal[1] + (vertex[2] + 4) * normal[2];
      expect(outward).toBeGreaterThan(0);
    });
  });
});

describe("scenery provider", () => {
  const content = (areaId: number | undefined) => ({
    scenery: () => JSON.stringify(exportJson()),
    areaAt: () => areaId,
    reliefAt: (x: number, z: number) => x + z,
  });

  test("routes area and relief queries to the WASM owners", () => {
    expect(createSceneryProvider(content(1)).areaAt(0, 0)?.name).toBe("Greyhaven Outpost");
    expect(createSceneryProvider(content(undefined)).areaAt(0, 0)).toBeNull();
    expect(() => createSceneryProvider(content(9)).areaAt(0, 0)).toThrow("missing");
    expect(createSceneryProvider(content(1)).reliefAt(1.4, 2.6)).toBe(4);
  });
});

describe("unit placement", () => {
  test("feet sit on flat physics ground raised by presentation relief", () => {
    const entity = { kind: "player" as const, entityId: 4, position: [250, 90, -100] as const, velocity: [0, 0, 0] as const, facing: 16_384 };
    expect(placeUnit(entity, 90, 0, 100)).toEqual({ x: 2.5, feetY: 0, z: -1, yawRadians: Math.PI / 2 });
    expect(placeUnit(entity, 90, 40, 100).feetY).toBeCloseTo(0.4, 9);
    const nodes = unitNodes("unit-player-4", placeUnit(entity, 90, 0, 100), {
      visuals: characterVisualProfile({ classId: "ranger", sex: "female" }),
      hat: "ranger-cap",
    });
    expect(new Set(nodes.map((node) => node.id)).size).toBe(nodes.length);
    expect(nodes.every((node) => node.id.startsWith("unit-player-4-"))).toBe(true);
    const nose = nodes.find((node) => node.id.endsWith("-nose"));
    expect(nose?.transform?.translation[0]).toBeGreaterThan(2.5);
  });
});
