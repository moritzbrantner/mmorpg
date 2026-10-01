import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { GRASS_PACKAGE_DIRECTORY, record } from "../scripts/grass-package";
import { encodeCommand } from "../src/command-wire";
import { decodeSnapshot, type EntityState } from "../src/replication";
import { BagState } from "../src/world/units/bag-state";
import { createLocalWorld } from "../src/world/local-world";
import { LocalZoneSource, type LocalZoneHandle } from "../src/world/local-zone-source";
import type { Prop } from "../src/world/scenery";
import { buildSceneryScene } from "../src/world/scenery-nodes";
import { terrainSurfaceY } from "../src/world/terrain-mesh";
import { localZoneModule } from "./support/local-zone-module";
import { worldSourceContract } from "./support/world-source-contract";

// Builds crates/mmorpg-wasm for wasm32 and runs wasm-bindgen, like `bun run build`.
const wasm = await localZoneModule();
const TICK_SECONDS = 1 / 30;
const EAST = 16_384;
const RUN_UNITS_PER_TICK = 21;

function self(source: LocalZoneSource): EntityState {
  const projection = source.latestProjection();
  const entity = projection?.entities.find((candidate) => candidate.kind === "player" && candidate.entityId === projection.viewerId);
  if (!entity) throw new Error("The viewer is missing from its projection");
  return entity;
}

function run(source: LocalZoneSource, ticks: number): void {
  for (let tick = 0; tick < ticks; tick += 1) {
    source.advance(TICK_SECONDS);
  }
}

describe("WASM local zone host", () => {
  test("multi-tick frames retain intermediate bag sheets and refusal feedback", () => {
    const { source } = createLocalWorld(wasm);
    source.join();
    const bag = new BagState();
    bag.update(source.latestProjection()!);
    source.sendCommand({ kind: "move-item", source: 0, destination: 15, quantity: 2 });
    const moved = source.advance(4 / 30);
    expect(moved.map((view) => view.tick)).toEqual([1n, 2n, 3n, 4n]);
    expect(moved[0]?.inventory).not.toBeNull();
    expect(moved[3]?.inventory).toBeNull();
    for (const view of moved) {
      bag.update(view);
    }
    expect(bag.ready).toBe(true);
    expect(bag.slots?.[15]).toEqual({ itemId: 1, quantity: 2 });
    source.sendCommand({ kind: "move-item", source: 15, destination: 1, quantity: 1 });
    const refused = source.advance(4 / 30);
    expect(refused[0]?.events).toContainEqual({ kind: "error", code: "invalid-inventory-move", target: null });
    expect(refused[3]?.events).toEqual([]);
    for (const view of refused) {
      bag.update(view);
    }
    expect(bag.feedback).toContain("refused");
    expect(source.advance(0)).toEqual([]);
  });
  test("bag moves use queued core authority and missed sheets recover periodically", () => {
    const { source } = createLocalWorld(wasm);
    source.join();
    const initial = source.latestProjection();
    expect(initial?.inventoryRevision).toBe(1n);
    expect(initial?.inventory?.[0]).toEqual({ itemId: 1, quantity: 3 });
    source.sendCommand({ kind: "move-item", source: 0, destination: 15, quantity: 2 });
    expect(source.latestProjection()).toEqual(initial);
    run(source, 1);
    const changed = source.latestProjection();
    expect(changed?.inventoryRevision).toBe(2n);
    expect(changed?.inventory?.[0]).toEqual({ itemId: 1, quantity: 1 });
    expect(changed?.inventory?.[15]).toEqual({ itemId: 1, quantity: 2 });
    source.sendCommand({ kind: "move-item", source: 15, destination: 1, quantity: 1 });
    run(source, 1);
    expect(source.latestProjection()?.events).toContainEqual({ kind: "error", code: "invalid-inventory-move", target: null });
    run(source, 7);
    expect(source.latestProjection()?.inventory).toBeNull();
    expect(source.latestProjection()?.inventoryRevision).toBe(2n);
    run(source, 1);
    expect(source.latestProjection()?.inventory).toEqual(changed?.inventory);
    source.leave();
    source.join();
    expect(source.latestProjection()?.inventoryRevision).toBe(1n);
    expect(source.latestProjection()?.inventory?.[0]).toEqual({ itemId: 1, quantity: 3 });
  });
  test("loads under Bun and hosts zone 1 with the shared content revision", () => {
    const zone = new wasm.LocalZone() as InstanceType<typeof wasm.LocalZone> & { zoneId(): number };
    expect(zone.zoneId()).toBe(1);
    expect(zone.contentRevision()).toBe(4n);
    const player = zone.join();
    const projection = decodeSnapshot(zone.projection(player));
    expect(projection).toMatchObject({ zoneId: 1, tick: 0n, contentRevision: 4n, viewerId: player });
    expect(projection.viewer).toEqual({
      health: 50, maxHealth: 50, experience: 0, experienceToNextLevel: 100, level: 1, dead: false, inCombat: false, autoAttacking: false, target: null,
    });
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
    const sampled = source.sample().find((entity) => entity.kind === "player" && entity.entityId === source.latestProjection()?.viewerId);
    expect(sampled?.position[0]).toBeGreaterThan(before - RUN_UNITS_PER_TICK);
    expect(sampled?.position[0]).toBeLessThan(before);
  });

  test("scenery and areas come from the same content as the zone", () => {
    const { source, scenery } = createLocalWorld(wasm);
    expect(scenery.scenery.contentRevision).toBe(4n);
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
    // The keep is its exact collider, and the lake is walkable water.
    const keep = scenery.scenery.props.find((prop) => prop.collider?.id === 100);
    expect(keep?.kind).toBe("keep");
    expect(keep?.collider?.halfExtents).toEqual(keep?.halfExtents);
    expect(scenery.scenery.water).toEqual([{ centerXz: [5_500, -5_500], radiiXz: [2_200, 1_600], surfaceY: 15 }]);
    expect(scenery.scenery.roads.map((road) => road.name)).toContain("Hollow Road");
    expect(Math.max(...scenery.scenery.farTerrain.heights)).toBeGreaterThan(Math.max(...scenery.scenery.terrain.heights));
  });

  test("WASM scenery consumes the exact selected Outpost anchors and presentation transforms", () => {
    const provider = createLocalWorld(wasm).scenery;
    const selected = record(JSON.parse(readFileSync(`${GRASS_PACKAGE_DIRECTORY}/selected.instances.json`, "utf8"))).instances;
    const transforms: unknown = JSON.parse(readFileSync(`${GRASS_PACKAGE_DIRECTORY}/selected.transforms.json`, "utf8"));
    if (!Array.isArray(selected) || !Array.isArray(transforms)) {
      throw new Error("Missing selected grass package");
    }
    const expected = selected.map((value, index) => {
      const position = record(value).positionMicro;
      const transform = record(transforms[index]);
      if (!Array.isArray(position)) {
        throw new Error("Missing selected grass coordinates");
      }
      const x = Number(position[0]) / 10_000;
      const y = Number(position[1]) / 10_000;
      const z = Number(position[2]) / 10_000 + 2000;
      return { kind: "grass-tuft", position: [x, y + provider.reliefAt(x, z), z], yaw: transform.yaw,
        scale: Number(transform.scalePermille) / 1000, halfExtents: transform.halfExtents, collider: null };
    });
    const actual = provider.scenery.props.filter((prop) => prop.kind === "grass-tuft" && prop.position[0] >= -3500 && prop.position[0] <= 3500 && prop.position[2] >= -1300 && prop.position[2] <= 5300);
    expect(actual).toEqual(expected);
    expect(actual.length).toBe(55);
    expect(provider.scenery.presentationFingerprint).toBe("bf866c8b4e7f837d");
    expect(provider.scenery.contentRevision).toBe(4n);
  });

  test("the vale's static scene models every prop within a bounded node and vertex budget", () => {
    const { scenery } = createLocalWorld(wasm);
    const scene = buildSceneryScene(scenery.scenery);
    const modelled = Object.values(scene.stats.props).reduce((sum, count) => sum + (count ?? 0), 0);
    expect(modelled).toBe(scenery.scenery.props.length);
    // Draw-call and per-frame validation budgets: ~4 200 props merge into a few hundred batches.
    expect(scene.stats.staticNodes).toBeLessThanOrEqual(600);
    expect(scene.stats.staticVertices).toBeLessThanOrEqual(230_000);
    expect(scenery.scenery.props.length).toBeGreaterThan(scene.stats.staticNodes * 5);
    expect(new Set(scene.batches.map((batch) => batch.node.geometry.resourceKey)).size).toBe(scene.batches.length);
    // Terrain and far-ring meshes span the whole vale, so none is ever culled: keep their palette small.
    const terrainMeshes = scene.batches.filter((batch) => /^static-all-(terrain|far):/.test(batch.node.id)).length;
    expect(terrainMeshes).toBeLessThanOrEqual(44);
  });

  test("props stand on the drawn terrain where the finer relief would lift them off it", () => {
    const vale = createLocalWorld(wasm).scenery.scenery;
    const { terrain, unitsPerMetre } = vale;
    const surfaceAt = (prop: Prop) => terrainSurfaceY(terrain, unitsPerMetre, prop.position[0] / unitsPerMetre, prop.position[2] / unitsPerMetre);
    // The prop of each kind whose exported feet sit highest above the 4 m mesh, if more than 10 cm.
    const worst = new Map<string, { prop: Prop; lift: number }>();
    for (const prop of vale.props) {
      const lift = prop.position[1] / unitsPerMetre - surfaceAt(prop);
      if (lift > 0.1 && lift > (worst.get(prop.kind)?.lift ?? 0)) {
        worst.set(prop.kind, { prop, lift });
      }
    }
    // Woodland relief has detail the 4 m grid cannot follow: trees, grass and flowers.
    expect([...worst.keys()]).toEqual(expect.arrayContaining(["tree-oak", "grass-tuft"]));
    for (const { prop } of worst.values()) {
      const lowest = Math.min(...buildSceneryScene({ ...vale, props: [prop] }).batches
        .filter((batch) => !batch.node.id.startsWith("static-all"))
        .flatMap((batch) => batch.node.geometry.positions.map(([, y]) => y)));
      expect(lowest, prop.kind).toBeLessThanOrEqual(surfaceAt(prop) + 0.02);
    }
  });
});

describe("WASM local zone combat intents", () => {
  test("the catalog names the hosted units and matches the zone's content", () => {
    const { catalog } = createLocalWorld(wasm);
    expect(catalog.contentRevision).toBe(4n);
    expect([...catalog.items.values()]).toEqual([
      { id: 1, name: "Torn Fur", maxStack: 20 }, { id: 2, name: "Worn Dagger", maxStack: 1 },
    ]);
    expect([...catalog.creatureTemplates.values()].map((template) => template.name)).toEqual([
      "Timber Wolf", "Young Boar", "Grain Rat", "Field Marauder", "Mirefin Lurker", "Redbrand Bandit", "Garrick Redbrand",
    ]);
    expect(catalog.npcs.get(5)).toEqual({ id: 5, name: "Brother Aldous", role: "spirit_healer", level: 10 });
    expect(catalog.areas.get(2)).toBe("Wolfrun Woods");
  });

  test("hub NPCs are visible, selectable and refuse to be attacked", () => {
    const { source, catalog } = createLocalWorld(wasm);
    source.join();
    const projection = source.latestProjection();
    const npcs = projection?.entities.filter((entity) => entity.kind === "npc") ?? [];
    expect(npcs.length).toBeGreaterThan(0);
    expect(projection?.entities.some((entity) => entity.kind === "creature")).toBe(false);
    const guard = npcs[0]!;
    expect(catalog.npcs.has(guard.entityId)).toBe(true);
    expect(guard.flags.attackable).toBe(false);
    const target = { kind: "npc", id: guard.entityId } as const;
    source.sendCommand({ kind: "select-target", target });
    source.sendCommand({ kind: "start-attack" });
    run(source, 1);
    const refused = source.latestProjection();
    expect(refused?.viewer.target).toEqual(target);
    expect(refused?.viewer.autoAttacking).toBe(false);
    expect(refused?.events).toEqual([{ kind: "error", code: "not-attackable", target }]);
    source.sendCommand({ kind: "release-spirit" });
    source.sendCommand({ kind: "select-target", target: null });
    run(source, 1);
    expect(source.latestProjection()?.events).toEqual([{ kind: "error", code: "not-dead", target: null }]);
    expect(source.latestProjection()?.viewer.target).toBeNull();
  });

  test("intents beyond the per-tick bound are reported, never a session error", () => {
    const { source } = createLocalWorld(wasm);
    source.join();
    for (let press = 0; press < 20; press += 1) {
      expect(() => source.sendCommand({ kind: "stop-attack" })).not.toThrow();
    }
    run(source, 1);
    expect(source.latestProjection()?.events).toEqual([{ kind: "error", code: "too-many-intents", target: null }]);
    source.sendCommand({ kind: "stop-attack" });
    run(source, 1);
    expect(source.latestProjection()?.events).toEqual([]);
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
