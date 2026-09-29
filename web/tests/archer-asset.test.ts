import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, test } from "bun:test";
import manifest from "../assets/medieval-character-kit/manifest.json";
import materials from "../assets/medieval-character-kit/materials.json";
import source from "../assets/medieval-character-kit/source.json";
import { ARCHER_MESHES, lowerArcherObj } from "../src/world/archer-asset";
import { characterLook } from "../src/world/humanoid";
import { PLAYER_MODEL, type UnitContext } from "../src/world/unit-nodes";
import { expectValidMesh } from "./support/geometry";
import { playerEntity } from "./support/snapshots";
import type { LocomotionState } from "../src/world/character-animation";

const obj = readFileSync(new URL("../assets/medieval-character-kit/archer.obj", import.meta.url));
const entry = manifest.assets[0]!;
const rest: LocomotionState = {
  forward: 0, right: 0, vertical: 0, moveWeight: 0, airWeight: 0, turnRate: 0, stridePhase: 0, shufflePhase: 0, time: 0,
};

describe("asset-tooling archer consumer", () => {
  test("the packaged OBJ has the declared upstream identity and exact topology", () => {
    expect(source).toEqual({
      repository: "moritzbrantner/asset-tooling", commit: "d0585174de734ba3e0ff407197dcdaa06a252c4d",
      generator: "examples/medieval-character-kit/build.ts", archetype: "archer",
    });
    expect(createHash("sha256").update(obj).digest("hex")).toBe(entry.sha256);
    expect(obj.length).toBe(entry.byteLength);
    expect(manifest.assets).toHaveLength(1);
    expect(ARCHER_MESHES.map(({ name }) => name)).toEqual(entry.parts);
    expect(Object.keys(materials.bindings)).toEqual(["archer"]);
    expect(ARCHER_MESHES.reduce((count, part) => count + part.geometry.indices.length / 3, 0)).toBe(entry.triangleCount);
    for (const part of ARCHER_MESHES) {
      expectValidMesh(part.geometry, part.name);
      expect(part.geometry.resourceKey).toBe(`asset-tooling:${entry.sha256}:${part.name}`);
    }
    expect(Math.min(...ARCHER_MESHES.flatMap((part) => part.geometry.positions.map((point) => point[1])))).toBe(0);
    expect(Math.max(...ARCHER_MESHES.flatMap((part) => part.geometry.positions.map((point) => point[1])))).toBeCloseTo(1.76, 3);
  });

  test("the viewer's ranger uses the packaged indexed meshes at the projected position and yaw", () => {
    const look = characterLook({ classId: "ranger", sex: "female" }, "ranger-cap");
    const entity = playerEntity(7, [250, 90, -100], [0, 0, 0], 16_384);
    const context = { viewerId: 7, viewerLook: look, playerHalfHeightUnits: 90 } as UnitContext;
    const frame = { id: "unit-player-7", entity, placement: { x: 2.5, feetY: 0.3, z: -1, yawRadians: Math.PI / 2 }, locomotion: rest, context };
    const nodes = PLAYER_MODEL.nodes(frame);
    expect(nodes.slice(0, entry.parts.length).map((node) => node.id)).toEqual(entry.parts.map((part) => `unit-player-7-${part}`));
    expect(nodes.slice(0, entry.parts.length).every((node) => node.geometry.kind === "mesh" && node.geometry.resourceKey.startsWith(`asset-tooling:${entry.sha256}:`))).toBe(true);
    expect(nodes.slice(0, entry.parts.length).every((node) => node.transform?.translation[0] === 2.5 && node.transform.translation[2] === -1)).toBe(true);
    expect(nodes.slice(0, entry.parts.length).every((node) => node.transform?.translation[1] === nodes[0]!.transform?.translation[1])).toBe(true);
    expect(nodes[0]!.transform?.translation[1]).toBeGreaterThan(0.29);
    expect(nodes.map((node) => node.id)).toContain("unit-player-7-cap-brim");
    expect(nodes.map((node) => node.id)).not.toContain("unit-player-7-bow-grip");
    expect(nodes[0]!.transform?.rotationQuaternion).toEqual([0, Math.sin(Math.PI / 4), 0, Math.cos(Math.PI / 4)]);
    expect(nodes.find((node) => node.id.endsWith("-torso"))?.color).toBe(look.visuals.bodyColor);
    expect(PLAYER_MODEL.nodes({ ...frame, locomotion: { ...rest, moveWeight: 1, forward: 6, time: 1 } }).map((node) => node.geometry)).toEqual(nodes.map((node) => node.geometry));
  });

  test("rejects unsupported or invalid OBJ topology", () => {
    expect(() => lowerArcherObj(obj.toString().replace(/^f \d+ \d+ \d+/m, "f 1 2 99999"))).toThrow();
    expect(() => lowerArcherObj(obj.toString().replace("g bow-string", "g missing"))).toThrow();
  });
});
