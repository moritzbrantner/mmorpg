import { describe, expect, test } from "bun:test";
import {
  PREVIEW_CHARACTER,
  enterPreviewWorld,
  initialEntryState,
} from "../src/character-selection.ts";

describe("character selection presentation gate", () => {
  test("starts behind character selection and enters the selected preview character", () => {
    const selection = initialEntryState();
    expect(selection).toEqual({
      phase: "character-selection",
      selectedCharacterId: PREVIEW_CHARACTER.id,
    });

    expect(enterPreviewWorld(selection)).toEqual({
      phase: "world",
      characterId: PREVIEW_CHARACTER.id,
    });
  });

  test("fails closed when the local selection is not in the preview roster", () => {
    expect(() =>
      enterPreviewWorld({ phase: "character-selection", selectedCharacterId: "unknown" }),
    ).toThrow("Selected character is not available");
  });

  test("keeps equipment ordering deterministic for the preview", () => {
    expect(PREVIEW_CHARACTER.equipment.map((item) => item.slot)).toEqual([
      "Head",
      "Shoulders",
      "Chest",
      "Hands",
      "Feet",
      "Main hand",
    ]);
  });
});
