import {
  MAX_CHARACTER_SLOTS,
  PREVIEW_CHARACTER,
  isCharacterClassId,
  isCharacterSex,
  localCharacterPreview,
  type CharacterPreview,
} from "./character-selection";

const ROSTER_SCHEMA_VERSION = 1;
const ROSTER_KEY = "mmorpg.offline-roster.v1";
const MAX_ROSTER_BYTES = 8192;

export type CharacterRosterStorage = Pick<Storage, "getItem" | "setItem">;

type StoredCharacter = {
  id: string;
  name: string;
  classId: unknown;
  sex: unknown;
};

function record(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Local character roster is invalid.");
  }
  const result = value as Record<string, unknown>;
  if (Object.keys(result).length !== keys.length || keys.some((key) => !Object.hasOwn(result, key))) {
    throw new Error("Local character roster has missing or unsupported fields.");
  }
  return result;
}

function decodeStoredCharacter(value: unknown): CharacterPreview {
  const stored = record(value, ["id", "name", "classId", "sex"]) as StoredCharacter;
  if (typeof stored.id !== "string" || typeof stored.name !== "string" ||
      !isCharacterClassId(stored.classId) || !isCharacterSex(stored.sex)) {
    throw new Error("Local character roster contains an invalid character.");
  }
  return localCharacterPreview(stored.id, stored.name, stored.classId, stored.sex);
}

export function characterRosterStorageKey(): string {
  return ROSTER_KEY;
}

export function loadCreatedCharacters(storage: CharacterRosterStorage): CharacterPreview[] {
  const raw = storage.getItem(ROSTER_KEY);
  if (raw === null) {
    return [];
  }
  if (raw.length > MAX_ROSTER_BYTES || new TextEncoder().encode(raw).byteLength > MAX_ROSTER_BYTES) {
    throw new Error("Local character roster is too large.");
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("Local character roster is not valid JSON.");
  }
  const envelope = record(parsed, ["schemaVersion", "characters"]);
  if (envelope.schemaVersion !== ROSTER_SCHEMA_VERSION || !Array.isArray(envelope.characters)) {
    throw new Error("Unsupported local character roster schema.");
  }
  if (envelope.characters.length > MAX_CHARACTER_SLOTS - 1) {
    throw new Error("Local character roster exceeds the available slots.");
  }

  const characters = envelope.characters.map(decodeStoredCharacter);
  const ids = new Set<string>();
  const names = new Set<string>();
  for (const character of characters) {
    const name = character.name.normalize("NFKC").toLowerCase();
    if (ids.has(character.id) || names.has(name) || character.id === PREVIEW_CHARACTER.id) {
      throw new Error("Local character roster contains duplicate identity.");
    }
    ids.add(character.id);
    names.add(name);
  }
  return characters;
}

export function saveCreatedCharacters(
  storage: CharacterRosterStorage,
  characters: readonly CharacterPreview[],
): void {
  if (characters.length > MAX_CHARACTER_SLOTS - 1) {
    throw new Error("Local character roster exceeds the available slots.");
  }
  const ids = new Set<string>();
  const names = new Set<string>();
  const stored = characters.map((character) => {
    const validated = localCharacterPreview(character.id, character.name, character.classId, character.sex);
    const name = validated.name.normalize("NFKC").toLowerCase();
    if (ids.has(validated.id) || names.has(name)) {
      throw new Error("Local character roster contains duplicate identity.");
    }
    ids.add(validated.id);
    names.add(name);
    return {
      id: validated.id,
      name: validated.name,
      classId: validated.classId,
      sex: validated.sex,
    };
  });
  const raw = JSON.stringify({ schemaVersion: ROSTER_SCHEMA_VERSION, characters: stored });
  if (new TextEncoder().encode(raw).byteLength > MAX_ROSTER_BYTES) {
    throw new Error("Local character roster is too large.");
  }
  storage.setItem(ROSTER_KEY, raw);
}
