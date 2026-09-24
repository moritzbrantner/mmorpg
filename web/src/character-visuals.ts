import type { CharacterClassId, CharacterPreview, CharacterSex } from "./character-selection";

export type CharacterWeaponStyle = "sword" | "bow" | "staff";

export type CharacterVisualProfile = {
  bodyRadius: number;
  bodyHeight: number;
  chestSize: readonly [number, number, number];
  shoulderSpan: number;
  armRadius: number;
  headRadius: number;
  bodyColor: `#${string}`;
  chestColor: `#${string}`;
  shoulderColor: `#${string}`;
  cloakColor: `#${string}`;
  weapon: CharacterWeaponStyle;
  weaponColor: `#${string}`;
};

const CLASS_VISUALS: Record<CharacterClassId, Omit<CharacterVisualProfile,
  "bodyRadius" | "bodyHeight" | "chestSize" | "shoulderSpan" | "armRadius" | "headRadius">> = {
  warden: {
    bodyColor: "#718d84",
    chestColor: "#58746b",
    shoulderColor: "#6e8f84",
    cloakColor: "#425c56",
    weapon: "sword",
    weaponColor: "#d3d6cf",
  },
  ranger: {
    bodyColor: "#748465",
    chestColor: "#617653",
    shoulderColor: "#788868",
    cloakColor: "#465a42",
    weapon: "bow",
    weaponColor: "#a88355",
  },
  arcanist: {
    bodyColor: "#777895",
    chestColor: "#62657e",
    shoulderColor: "#8580a0",
    cloakColor: "#4d4d69",
    weapon: "staff",
    weaponColor: "#bd895f",
  },
};

const SEX_FRAME: Record<CharacterSex, Pick<CharacterVisualProfile,
  "bodyRadius" | "bodyHeight" | "chestSize" | "shoulderSpan" | "armRadius" | "headRadius">> = {
  male: {
    bodyRadius: 0.48,
    bodyHeight: 1.7,
    chestSize: [0.86, 0.82, 0.5],
    shoulderSpan: 0.52,
    armRadius: 0.16,
    headRadius: 0.34,
  },
  female: {
    bodyRadius: 0.44,
    bodyHeight: 1.66,
    chestSize: [0.79, 0.8, 0.48],
    shoulderSpan: 0.47,
    armRadius: 0.15,
    headRadius: 0.33,
  },
};

export function characterVisualProfile(
  character: Pick<CharacterPreview, "classId" | "sex">,
): CharacterVisualProfile {
  const classVisuals = CLASS_VISUALS[character.classId];
  const frame = SEX_FRAME[character.sex];
  return {
    ...frame,
    ...classVisuals,
    chestSize: [frame.chestSize[0], frame.chestSize[1], frame.chestSize[2]],
  };
}
