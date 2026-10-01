import type { EntityFlags, EntityState, ViewerState, Vector3, ZoneSnapshot } from "../../src/replication";

export const NO_FLAGS: EntityFlags = {
  dead: false, inCombat: false, hostile: false, attackable: false, tappedByOther: false, evading: false, targetsViewer: false,
};

/** A healthy level-1 player record. */
export function playerEntity(entityId: number, position: Vector3, velocity: Vector3 = [0, 0, 0], facing = 0): EntityState {
  return { kind: "player", entityId, appearance: 0, position, velocity, facing, level: 1, healthPercent: 100, flags: NO_FLAGS };
}

export const HEALTHY_VIEWER: ViewerState = {
  health: 50, maxHealth: 50, experience: 0, experienceToNextLevel: 100, level: 1, dead: false, inCombat: false, autoAttacking: false, target: null,
};

/** A projection with a healthy viewer, no target and no events. */
export function testSnapshot(
  fields: Pick<ZoneSnapshot, "zoneId" | "tick" | "contentRevision" | "acknowledgedSequence" | "viewerId" | "entities"> &
    Partial<Pick<ZoneSnapshot, "viewer" | "targetOfTarget" | "events">>,
): ZoneSnapshot {
  return { viewer: HEALTHY_VIEWER, targetOfTarget: null, events: [], ...fields };
}
