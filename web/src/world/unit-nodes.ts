import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { rotateYawOffset, type HatStyle } from "../character-customization";
import type { CharacterVisualProfile } from "../character-visuals";
import type { EntityState } from "../replication";

type Quaternion = [number, number, number, number];

/** How one visible unit looks. Projections carry no appearance yet, so other players share one look. */
export type UnitLook = { visuals: CharacterVisualProfile; hat: HatStyle | null };

/** Where a unit stands in metres: its feet, raised by presentation relief. */
export type UnitPlacement = { x: number; feetY: number; z: number; yawRadians: number };

const YAW_STEPS = 65_536;
/** Body centre above the feet, matching the character selection proportions. */
const BODY_CENTRE_METRES = 0.83;

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

/** Renderer nodes for one unit; IDs are unique per unit so several players can share a frame. */
export function unitNodes(id: string, placement: UnitPlacement, look: UnitLook): RendererSceneNode[] {
  const { x, z, yawRadians: yaw } = placement;
  const y = placement.feetY + BODY_CENTRE_METRES;
  const { visuals } = look;
  const rotationQuaternion: Quaternion = [0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)];
  const [noseX, noseZ] = rotateYawOffset(yaw, 0, 0.36);
  return [
    {
      id: `${id}-body`,
      geometry: { kind: "cylinder", radius: visuals.bodyRadius * 0.88, height: visuals.bodyHeight * 0.97 },
      color: visuals.bodyColor,
      transform: { translation: [x, y, z], rotationQuaternion },
    },
    {
      id: `${id}-head`,
      geometry: { kind: "sphere", radius: visuals.headRadius },
      color: "#c49b78",
      transform: { translation: [x, y + 1.07, z] },
    },
    {
      id: `${id}-nose`,
      geometry: { kind: "sphere", radius: 0.08 },
      color: "#b58a68",
      transform: { translation: [x + noseX, y + 1.07, z + noseZ] },
    },
    {
      id: `${id}-shoulders`,
      geometry: { kind: "box", size: [visuals.shoulderSpan * 2.08, 0.24, 0.42] },
      color: visuals.shoulderColor,
      transform: { translation: [x, y + 0.56, z], rotationQuaternion },
    },
    ...weaponNodes(id, x, y, z, yaw, rotationQuaternion, visuals),
    ...(look.hat === null ? [] : hatNodes(id, x, y, z, rotationQuaternion, look.hat)),
  ];
}

function weaponNodes(
  id: string,
  x: number,
  y: number,
  z: number,
  yaw: number,
  rotationQuaternion: Quaternion,
  visuals: CharacterVisualProfile,
): RendererSceneNode[] {
  const [weaponX, weaponZ] = rotateYawOffset(yaw, 0.58, 0.04);
  switch (visuals.weapon) {
    case "bow": {
      const [stringX, stringZ] = rotateYawOffset(yaw, 0.48, 0.04);
      return [
        {
          id: `${id}-bow`,
          geometry: { kind: "box", size: [0.09, 1.45, 0.08] },
          color: visuals.weaponColor,
          transform: { translation: [x + weaponX, y + 0.15, z + weaponZ], rotationQuaternion },
        },
        {
          id: `${id}-bow-string`,
          geometry: { kind: "box", size: [0.025, 1.32, 0.025] },
          color: "#d8d5c7",
          transform: { translation: [x + stringX, y + 0.15, z + stringZ], rotationQuaternion },
        },
      ];
    }
    case "staff":
      return [
        {
          id: `${id}-staff`,
          geometry: { kind: "box", size: [0.1, 1.75, 0.1] },
          color: visuals.weaponColor,
          transform: { translation: [x + weaponX, y + 0.18, z + weaponZ], rotationQuaternion },
        },
        {
          id: `${id}-staff-focus`,
          geometry: { kind: "sphere", radius: 0.19 },
          color: "#d6a677",
          transform: { translation: [x + weaponX, y + 1.07, z + weaponZ] },
        },
      ];
    case "sword":
      return [{
        id: `${id}-sword`,
        geometry: { kind: "box", size: [0.12, 1.35, 0.08] },
        color: visuals.weaponColor,
        transform: { translation: [x + weaponX, y + 0.15, z + weaponZ], rotationQuaternion },
      }];
  }
}

function hatNodes(
  id: string,
  x: number,
  y: number,
  z: number,
  rotationQuaternion: Quaternion,
  hat: HatStyle,
): RendererSceneNode[] {
  switch (hat) {
    case "wayfarer-hood":
      return [{
        id: `${id}-hat-hood`,
        geometry: { kind: "cylinder", radius: 0.36, height: 0.2 },
        color: "#b7c6bd",
        transform: { translation: [x, y + 1.34, z], rotationQuaternion },
      }];
    case "ranger-cap":
      return [
        {
          id: `${id}-hat-cap-brim`,
          geometry: { kind: "cylinder", radius: 0.45, height: 0.08 },
          color: "#6f875f",
          transform: { translation: [x, y + 1.32, z], rotationQuaternion },
        },
        {
          id: `${id}-hat-cap-crown`,
          geometry: { kind: "cylinder", radius: 0.29, height: 0.2 },
          color: "#5d7351",
          transform: { translation: [x, y + 1.42, z], rotationQuaternion },
        },
      ];
    case "ironcrest-helm":
      return [
        {
          id: `${id}-hat-helm`,
          geometry: { kind: "cylinder", radius: 0.35, height: 0.28 },
          color: "#858e91",
          transform: { translation: [x, y + 1.33, z], rotationQuaternion },
        },
        {
          id: `${id}-hat-crest`,
          geometry: { kind: "box", size: [0.1, 0.36, 0.34] },
          color: "#aab0b2",
          transform: { translation: [x, y + 1.58, z], rotationQuaternion },
        },
      ];
  }
}
