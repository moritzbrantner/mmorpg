import { describe, expect, test } from "bun:test";
import { encodeCommand } from "../src/command-wire";
import { decodeSnapshot, type EntityState } from "../src/replication";
import { createLocalWorld } from "../src/world/local-world";
import { LocalZoneSource, type LocalZoneHandle } from "../src/world/local-zone-source";
import { buildSceneryNodes } from "../src/world/scenery-nodes";
import { localZoneModule } from "./support/local-zone-module";
import { worldSourceContract } from "./support/world-source-contract";

// Builds crates/mmorpg-wasm for wasm32 and runs wasm-bindgen, like `bun run build`.
const wasm = await localZoneModule();
const TICK_SECONDS = 1 / 30;
const EAST = 16_384;
const RUN_UNITS_PER_TICK = 21;

function self(source: LocalZoneSource): EntityState {
  const projection = source.latestProjection();
  const entity = projection?.entities.find((candidate) => candidate.entityId === projection.viewerId);
  if (!entity) throw new Error("The viewer is missing from its projection");
  return entity;
}

function run(source: LocalZoneSource, ticks: number): void {
  for (let tick = 0; tick < ticks; tick += 1) {
    source.advance(TICK_SECONDS);
  }
}

describe("WASM local zone host", () => {
  test("loads under Bun and hosts zone 1 with the shared content revision", () => {
    const zone = new wasm.LocalZone() as InstanceType<typeof wasm.LocalZone> & { zoneId(): number };
    expect(zone.zoneId()).toBe(1);
    expect(zone.contentRevision()).toBe(2n);
    const player = zone.join();
    const projection = decodeSnapshot(zone.projection(player));
    expect(projection).toMatchObject({ zoneId: 1, tick: 0n, contentRevision: 2n, viewerId: player });
    expect(() => zone.submit(player, 1, Uint8Array.of(1, 1, 0, 0, 0, 0))).toThrow();
    expect(zone.submit(player, 1, encodeCommand({ kind: "jump" }))).toBe(true);
    expect(zone.submit(player, 1, encodeCommand({ kind: "jump" }))).toBe(false);
    expect(zone.leave(player)).toBe(true);
    expect(() => zone.projection(player)).toThrow();
  });

  test("camera-relative movement moves the decoded viewer in the expected direction", () => {
    const { source } = createLocalWorld(wasm);
    source.join();
    const spawn = self(source).position;
    source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: EAST });
    run(source, 10);
    const east = self(source).position;
    expect(source.latestProjection()?.tick).toBe(10n);
    expect(east).toEqual([spawn[0] + 10 * RUN_UNITS_PER_TICK, spawn[1], spawn[2]]);
    expect(self(source).facing).toBe(EAST);

    // The character's right is yaw - 90°: facing +X, strafing right heads toward +Z.
    source.sendCommand({ kind: "move", forward: 0, strafe: 1, facing: EAST });
    run(source, 5);
    const right = self(source).position;
    expect(right[0]).toBe(east[0]);
    expect(right[2]).toBe(east[2] + 5 * RUN_UNITS_PER_TICK);

    source.sendCommand({ kind: "move", forward: 0, strafe: 0, facing: EAST });
    run(source, 3);
    expect(self(source).position).toEqual(right);
  });

  test("a grounded jump rises and physics brings the unit back down", () => {
    const { source } = createLocalWorld(wasm);
    source.join();
    const ground = self(source).position[1];
    source.sendCommand({ kind: "jump" });
    run(source, 1);
    const lifted = self(source).position[1];
    expect(lifted).toBeGreaterThan(ground);
    let apex = lifted;
    for (let tick = 0; tick < 60; tick += 1) {
      run(source, 1);
      apex = Math.max(apex, self(source).position[1]);
    }
    expect(apex).toBeGreaterThan(lifted);
    expect(self(source).position[1]).toBe(ground);
  });

  test("interpolated samples stay between authoritative ticks", () => {
    const { source } = createLocalWorld(wasm);
    source.join();
    source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: EAST });
    run(source, 2);
    const before = self(source).position[0];
    source.advance(TICK_SECONDS / 2);
    const sampled = source.sample().find((entity) => entity.entityId === source.latestProjection()?.viewerId);
    expect(sampled?.position[0]).toBeGreaterThan(before - RUN_UNITS_PER_TICK);
    expect(sampled?.position[0]).toBeLessThan(before);
  });

  test("scenery and areas come from the same content as the zone", () => {
    const { source, scenery } = createLocalWorld(wasm);
    expect(scenery.scenery.contentRevision).toBe(2n);
    expect(scenery.scenery.source).toBe("mmorpg-scenery");
    expect(scenery.scenery.playerHalfExtents).toEqual([30, 90, 30]);
    source.join();
    const [x, , z] = self(source).position;
    expect(scenery.areaAt(x, z)?.name).toBe("Greyhaven Outpost");
    expect(scenery.areaAt(-5_500, 0)?.name).toBe("Wolfrun Woods");
    expect(scenery.areaAt(-50_000, -50_000)).toBeNull();
    // Spawn slots are flat; the mountains beyond the walls are not.
    expect(scenery.reliefAt(x, z)).toBe(0);
    expect(scenery.reliefAt(0, -19_000)).toBeGreaterThan(1_000);
    // The keep's block is its collider, and the lake is walkable water.
    expect(scenery.scenery.props.some((prop) => prop.colliderId === 100)).toBe(true);
    expect(scenery.scenery.water).toEqual([{ centerXz: [5_500, -5_500], radiiXz: [2_200, 1_600], surfaceY: 15 }]);
    // One terrain mesh per biome present, one prop mesh per colour: the node
    // count stays small however many props the scenery carries.
    const nodes = buildSceneryNodes(scenery.scenery);
    const biomes = new Set(scenery.scenery.terrain.biomes).size;
    const colours = new Set(scenery.scenery.props.map((prop) => prop.color)).size;
    expect(nodes.filter((node) => node.id.startsWith("terrain-")).length).toBe(biomes);
    expect(nodes.length).toBe(biomes + colours + scenery.scenery.water.length);
    expect(scenery.scenery.props.length).toBeGreaterThan(nodes.length * 10);
  });
});

/** The real WASM zone, but its projections lose their first byte until repaired. */
function refusingWasmSource() {
  const zone = new wasm.LocalZone();
  let corrupt = true;
  const handle: LocalZoneHandle = {
    join: () => zone.join(),
    leave: (player) => zone.leave(player),
    submit: (player, sequence, command) => zone.submit(player, sequence, command),
    tick: () => zone.tick(),
    projection: (player) => (corrupt ? zone.projection(player).subarray(1) : zone.projection(player)),
  };
  return { source: new LocalZoneSource(handle), repair: () => { corrupt = false; } };
}

worldSourceContract("LocalZoneSource over the WASM zone", () => createLocalWorld(wasm).source, refusingWasmSource);
