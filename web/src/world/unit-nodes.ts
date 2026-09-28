import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { EntityKind, EntityState } from "../replication";
import { UnitAnimator, poseFor, type LocomotionState } from "./character-animation";
import { humanoidNodes, humanoidStance, type HumanoidLook, type UnitPlacement } from "./humanoid";

/**
 * Unit rendering by entity kind. A registry maps each projected entity kind
 * to a model; players are animated humanoids. Creature and NPC kinds (step 7)
 * plug in here with their own models keyed by kind and appearance, and the
 * render loop stays unchanged.
 */
export type { UnitPlacement } from "./humanoid";

/** How one visible unit looks. Projections carry no appearance yet, so other players share one look. */
export type UnitLook = HumanoidLook;

/** Everything a unit model draws from in one frame. */
export type UnitFrame = { id: string; placement: UnitPlacement; locomotion: LocomotionState; look: UnitLook };

export type UnitModel = { nodes(frame: UnitFrame): RendererSceneNode[] };

const YAW_STEPS = 65_536;
/** Presentation velocity arrives in units per tick. */
const TICKS_PER_SECOND = 30;

export function yawRadians(facing: number): number {
  return (facing / YAW_STEPS) * 2 * Math.PI;
}

/**
 * Converts a projected entity (body centre in units) into a placement. The
 * physics body centre sits `halfHeight` above the feet; relief is added on
 * top because walkable ground is physically flat.
 */
export function placeUnit(
  entity: EntityState,
  halfHeightUnits: number,
  reliefUnits: number,
  unitsPerMetre: number,
): UnitPlacement {
  return {
    x: entity.position[0] / unitsPerMetre,
    feetY: (entity.position[1] - halfHeightUnits + reliefUnits) / unitsPerMetre,
    z: entity.position[2] / unitsPerMetre,
    yawRadians: yawRadians(entity.facing),
  };
}

export const HUMANOID_MODEL: UnitModel = {
  nodes: (frame) => humanoidNodes(frame.id, frame.placement, frame.look, poseFor(frame.locomotion, humanoidStance(frame.look))),
};

/** Models by entity kind; adding a kind to `EntityKind` requires an entry here. */
export const UNIT_MODELS: { readonly [kind in EntityKind]: UnitModel } = {
  player: HUMANOID_MODEL,
};

export function unitNodes(kind: EntityKind, frame: UnitFrame): RendererSceneNode[] {
  return UNIT_MODELS[kind].nodes(frame);
}

export function unitIdentity(entity: Pick<EntityState, "kind" | "entityId">): string {
  return `unit-${entity.kind}-${entity.entityId}`;
}

/** Animation state per visible unit, dropped when the unit leaves the projection. */
export class UnitAnimators {
  readonly #animators = new Map<string, UnitAnimator>();

  /** Locomotion for this frame; `instant` snaps blends (reduced motion). */
  locomotion(entity: EntityState, placement: UnitPlacement, unitsPerMetre: number, deltaSeconds: number, instant: boolean): LocomotionState {
    const identity = unitIdentity(entity);
    let animator = this.#animators.get(identity);
    if (!animator) {
      animator = new UnitAnimator();
      this.#animators.set(identity, animator);
    }
    const scale = TICKS_PER_SECOND / unitsPerMetre;
    return animator.update({
      x: placement.x,
      z: placement.z,
      velocity: [entity.velocity[0] * scale, entity.velocity[1] * scale, entity.velocity[2] * scale],
      facing: placement.yawRadians,
    }, deltaSeconds, instant);
  }

  /** Forgets units that are no longer visible. */
  retain(identities: ReadonlySet<string>): void {
    for (const identity of this.#animators.keys()) {
      if (!identities.has(identity)) {
        this.#animators.delete(identity);
      }
    }
  }

  clear(): void {
    this.#animators.clear();
  }
}
