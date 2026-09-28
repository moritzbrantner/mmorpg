import { describe, expect, test } from "bun:test";
import { OTHER_PLAYER_LOOK } from "../src/world/humanoid";
import { UNIT_MODELS, UnitAnimators, placeUnit, unitIdentity, unitNodes } from "../src/world/unit-nodes";

describe("unit placement", () => {
  test("feet sit on flat physics ground raised by presentation relief", () => {
    const entity = { kind: "player" as const, entityId: 4, position: [250, 90, -100] as const, velocity: [0, 0, 0] as const, facing: 16_384 };
    expect(placeUnit(entity, 90, 0, 100)).toEqual({ x: 2.5, feetY: 0, z: -1, yawRadians: Math.PI / 2 });
    expect(placeUnit(entity, 90, 40, 100).feetY).toBeCloseTo(0.4, 9);
    expect(unitIdentity(entity)).toBe("unit-player-4");
  });
});

describe("unit model registry", () => {
  test("every entity kind has a model and animation state is kept per visible unit", () => {
    expect(Object.keys(UNIT_MODELS)).toEqual(["player"]);
    const animators = new UnitAnimators();
    const entity = { kind: "player" as const, entityId: 3, position: [0, 90, 0] as const, velocity: [21, 0, 0] as const, facing: 16_384 };
    const first = placeUnit(entity, 90, 0, 100);
    animators.locomotion(entity, first, 100, 1 / 30, true);
    const moved = { ...entity, position: [21, 90, 0] as const };
    const locomotion = animators.locomotion(moved, placeUnit(moved, 90, 0, 100), 100, 1 / 30, true);
    expect(locomotion.forward).toBeCloseTo(6.3, 9);
    expect(locomotion.stridePhase).toBeGreaterThan(0);
    animators.retain(new Set());
    const fresh = animators.locomotion(moved, placeUnit(moved, 90, 0, 100), 100, 1 / 30, true);
    expect(fresh.stridePhase).toBe(0);
    const nodes = unitNodes("player", { id: "unit-player-3", placement: first, locomotion, look: OTHER_PLAYER_LOOK });
    expect(nodes.length).toBeGreaterThan(20);
  });
});
