import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CHARACTER_CLASSES,
  MAX_CHARACTER_SLOTS,
  PREVIEW_CHARACTER,
  createCharacterPreview,
  draftCharacterPreview,
  localCharacterPreview,
} from "../src/character-selection";
import {
  characterRosterStorageKey,
  loadCreatedCharacters,
  saveCreatedCharacters,
} from "../src/character-roster";
import { characterVisualProfile } from "../src/character-visuals";

function memory() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
}

test("offers exactly three classes with distinct starter roles and equipment", () => {
  assert.deepEqual(CHARACTER_CLASSES.map(({ id, name }) => ({ id, name })), [
    { id: "warden", name: "Warden" },
    { id: "ranger", name: "Ranger" },
    { id: "arcanist", name: "Arcanist" },
  ]);
  for (const definition of CHARACTER_CLASSES) {
    assert.ok(definition.summary.length > 0);
    assert.equal(definition.starterEquipment.length, 6);
    assert.equal(definition.starterEquipment.at(-1)?.slot, "Main hand");
  }
});

test("creates male and female level-one characters with stable local identities", () => {
  const alina = createCharacterPreview({ name: "  Alina   Grey  ", sex: "female", classId: "ranger" }, [PREVIEW_CHARACTER]);
  assert.equal(alina.id, "local-1");
  assert.equal(alina.name, "Alina Grey");
  assert.equal(alina.sex, "female");
  assert.equal(alina.className, "Ranger");
  assert.equal(alina.level, 1);

  const bran = createCharacterPreview({ name: "Bran", sex: "male", classId: "arcanist" }, [PREVIEW_CHARACTER, alina]);
  assert.equal(bran.id, "local-2");
  assert.equal(bran.sex, "male");
  assert.equal(bran.className, "Arcanist");
});

test("draft preview never allocates identity and reflects class/sex immediately", () => {
  const draft = draftCharacterPreview({ name: "", sex: "female", classId: "arcanist" });
  assert.equal(draft.id, "draft");
  assert.equal(draft.name, "New character");
  assert.equal(draft.sex, "female");
  assert.equal(draft.className, "Arcanist");
  assert.equal(draft.equipment.at(-1)?.name, "Emberglass Staff");
});

test("rejects invalid, duplicate, and exhausted character creation", () => {
  for (const name of ["", "A", "1mage", "Bad_Name", "x".repeat(25)]) {
    assert.throws(() => createCharacterPreview({ name, sex: "male", classId: "warden" }, [PREVIEW_CHARACTER]));
  }
  assert.throws(() => createCharacterPreview(
    { name: "aELRIC sTORMWARD", sex: "female", classId: "ranger" },
    [PREVIEW_CHARACTER],
  ), /already exists/);

  const full = [PREVIEW_CHARACTER];
  for (let slot = 1; slot < MAX_CHARACTER_SLOTS; slot += 1) {
    full.push(localCharacterPreview(`local-${slot}`, `Hero ${String.fromCharCode(64 + slot)}`, "warden", "male"));
  }
  assert.throws(() => createCharacterPreview(
    { name: "Overflow", sex: "female", classId: "arcanist" },
    full,
  ), /slots are full/);
});

test("created roster round-trips without storing the built-in character", () => {
  const storage = memory();
  const one = createCharacterPreview({ name: "Alina", sex: "female", classId: "ranger" }, [PREVIEW_CHARACTER]);
  const two = createCharacterPreview({ name: "Dorian", sex: "male", classId: "arcanist" }, [PREVIEW_CHARACTER, one]);
  saveCreatedCharacters(storage, [one, two]);

  const loaded = loadCreatedCharacters(storage);
  assert.deepEqual(loaded, [one, two]);
  assert.ok(storage.values.get(characterRosterStorageKey())?.includes('"local-1"'));
  assert.ok(!storage.values.get(characterRosterStorageKey())?.includes(PREVIEW_CHARACTER.id));
});

test("roster parsing fails closed for corrupt schema, duplicate identity, and invalid class/sex", () => {
  const storage = memory();
  for (const raw of [
    "{",
    JSON.stringify({ schemaVersion: 2, characters: [] }),
    JSON.stringify({ schemaVersion: 1, characters: [{ id: "local-1", name: "Alina", classId: "mage", sex: "female" }] }),
    JSON.stringify({ schemaVersion: 1, characters: [{ id: "local-1", name: "Alina", classId: "ranger", sex: "other" }] }),
    JSON.stringify({ schemaVersion: 1, characters: [{ id: "local-1", name: "AELRIC STORMWARD", classId: "ranger", sex: "female" }] }),
    JSON.stringify({ schemaVersion: 1, characters: [
      { id: "local-1", name: "Alina", classId: "ranger", sex: "female" },
      { id: "local-1", name: "Dorian", classId: "warden", sex: "male" },
    ] }),
  ]) {
    storage.values.set(characterRosterStorageKey(), raw);
    assert.throws(() => loadCreatedCharacters(storage));
  }
});

test("sex affects presentation geometry while class changes palette and main-hand style", () => {
  const femaleRanger = characterVisualProfile({ sex: "female", classId: "ranger" });
  const maleRanger = characterVisualProfile({ sex: "male", classId: "ranger" });
  assert.notEqual(femaleRanger.shoulderSpan, maleRanger.shoulderSpan);
  assert.equal(femaleRanger.weapon, "bow");

  const warden = characterVisualProfile({ sex: "female", classId: "warden" });
  const arcanist = characterVisualProfile({ sex: "female", classId: "arcanist" });
  assert.equal(warden.weapon, "sword");
  assert.equal(arcanist.weapon, "staff");
  assert.notEqual(warden.chestColor, arcanist.chestColor);
});
