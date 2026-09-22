export type EquipmentItem = {
  slot: string;
  name: string;
  accent: string;
};

export type CharacterPreview = {
  id: string;
  name: string;
  race: string;
  className: string;
  level: number;
  location: string;
  equipment: readonly EquipmentItem[];
};

export const PREVIEW_CHARACTER: CharacterPreview = {
  id: "aelric-stormward",
  name: "Aelric Stormward",
  race: "Human",
  className: "Warden",
  level: 18,
  location: "Greyhaven Outpost",
  equipment: [
    { slot: "Head", name: "Wayfarer's Hood", accent: "#b7c6bd" },
    { slot: "Shoulders", name: "Greywatch Mantle", accent: "#6e8f84" },
    { slot: "Chest", name: "Warden's Brigandine", accent: "#718d84" },
    { slot: "Hands", name: "Ashwood Grips", accent: "#8b7255" },
    { slot: "Feet", name: "Trailbound Boots", accent: "#745d49" },
    { slot: "Main hand", name: "Oathkeeper Blade", accent: "#c7b66d" },
  ],
};

export type EntryState =
  | { phase: "character-selection"; selectedCharacterId: string }
  | { phase: "world"; characterId: string };

export function initialEntryState(character: CharacterPreview = PREVIEW_CHARACTER): EntryState {
  return { phase: "character-selection", selectedCharacterId: character.id };
}

/**
 * Presentation-only transition for the offline browser demo.
 * Online entry must still authenticate the durable character and acquire a server route.
 */
export function enterPreviewWorld(
  state: EntryState,
  character: CharacterPreview = PREVIEW_CHARACTER,
): EntryState {
  if (state.phase === "world") return state;
  if (state.selectedCharacterId !== character.id) {
    throw new Error("Selected character is not available in this preview roster");
  }
  return { phase: "world", characterId: character.id };
}
