import { describe, expect, test } from "bun:test";
import type { EntityState } from "../src/replication";
import { OTHER_PLAYER_LOOK, characterLook } from "../src/world/humanoid";
import {
  PLAYER_MODEL,
  UNIT_MODELS,
  UnitAnimators,
  placeUnit,
  placeWithModel,
  unitIdentity,
  unitModel,
  type UnitContext,
  type UnitModel,
} from "../src/world/unit-nodes";

const CONTEXT: UnitContext = {
  unitsPerMetre: 100,
  playerHalfHeightUnits: 90,
  viewerId: 3,
  viewerLook: characterLook({ classId: "ranger", sex: "female" }, "ranger-cap"),
};

function player(entityId: number, position: EntityState["position"] = [0, 90, 0]): EntityState {
  return { kind: "player", entityId, position, velocity: [0, 0, 0], facing: 0 };
}

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
    expect(unitModel(player(3))).toBe(PLAYER_MODEL);
    const animators = new UnitAnimators();
    const entity = { ...player(3), velocity: [21, 0, 0] as const, facing: 16_384 };
    const first = placeUnit(entity, 90, 0, 100);
    animators.locomotion(entity, first, 100, 1 / 30, true);
    const moved = { ...entity, position: [21, 90, 0] as const };
    const locomotion = animators.locomotion(moved, placeUnit(moved, 90, 0, 100), 100, 1 / 30, true);
    expect(locomotion.forward).toBeCloseTo(6.3, 9);
    expect(locomotion.stridePhase).toBeGreaterThan(0);
    animators.retain(new Set());
    const fresh = animators.locomotion(moved, placeUnit(moved, 90, 0, 100), 100, 1 / 30, true);
    expect(fresh.stridePhase).toBe(0);
    const nodes = PLAYER_MODEL.nodes({ id: "unit-player-3", entity, placement: first, locomotion, context: CONTEXT });
    expect(nodes.length).toBeGreaterThan(20);
  });

  test("the player model dresses the viewer's own unit in the local look and others in the shared one", () => {
    const placement = placeWithModel(PLAYER_MODEL, player(3), CONTEXT, 0);
    expect(placement.feetY).toBe(0);
    const locomotion = new UnitAnimators().locomotion(player(3), placement, 100, 1 / 30, true);
    const draw = (entityId: number, viewerLook = CONTEXT.viewerLook) =>
      PLAYER_MODEL.nodes({ id: "unit", entity: player(entityId), placement, locomotion, context: { ...CONTEXT, viewerLook } });
    expect(draw(3)).not.toEqual(draw(8));
    expect(draw(3, OTHER_PLAYER_LOOK)).toEqual(draw(8));
  });

  test("models read the projected entity: registry dispatch, per-entity body height and nodes", () => {
    // A stand-in for a creature model keyed by appearance: its body height depends on the entity.
    const heights = new Map([[5, 40], [6, 120]]);
    const seen: EntityState[] = [];
    const sized: UnitModel = {
      halfHeightUnits: (entity) => heights.get(entity.entityId) ?? 0,
      nodes: (frame) => {
        seen.push(frame.entity);
        return [{ id: `${frame.id}-body`, geometry: { kind: "sphere", radius: 0.3 }, color: "#808080", transform: { translation: [0, 0, 0] } }];
      },
    };
    const registry = { player: sized };
    const small = player(5, [100, 40, 0]);
    const tall = player(6, [100, 120, 0]);
    expect(unitModel(small, registry)).toBe(sized);
    // Both stand on the ground whatever their size, raised by the same relief.
    expect(placeWithModel(sized, small, CONTEXT, 25).feetY).toBeCloseTo(0.25, 9);
    expect(placeWithModel(sized, tall, CONTEXT, 25).feetY).toBeCloseTo(0.25, 9);
    // The player's half height would sink the small body into the ground.
    expect(placeWithModel(PLAYER_MODEL, small, CONTEXT, 25).feetY).toBeCloseTo(-0.25, 9);
    const placement = placeWithModel(sized, tall, CONTEXT, 0);
    const locomotion = new UnitAnimators().locomotion(tall, placement, 100, 1 / 30, true);
    sized.nodes({ id: unitIdentity(tall), entity: tall, placement, locomotion, context: CONTEXT });
    expect(seen).toEqual([tall]);
  });
});
