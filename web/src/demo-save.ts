/** Offline demo persistence only. Never submit these files to an online zone host. */
import { isHatStyle, type CharacterAppearance, type CharacterStorage } from "./character-customization";
import { MAX_DEMO_TICK } from "./demo-clock";

export const DEMO_WORLD_ID = "greyhaven-outpost-v1";
export const DEMO_WORLD_HALF_EXTENT = 10.5;
export const MAX_DEMO_SAVE_BYTES = 8192;
const FORMAT = "mmorpg.offline-demo-save";
const VERSION = 1;

export type DemoProgress = {
  position: { x: number; z: number };
  facing: number;
  waystoneActive: boolean;
  appearance: CharacterAppearance;
  tick: bigint;
  tickFraction: number;
};

export type DemoSaveFile = Pick<File, "size" | "text">;

function record(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Save data must contain objects, not arrays or null.");
  }
  const result = value as Record<string, unknown>;
  if (Object.keys(result).length !== keys.length || keys.some((key) => !Object.hasOwn(result, key))) {
    throw new Error("Save data has missing or unsupported fields.");
  }
  return result;
}

function numberInRange(value: unknown, min: number, max: number): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < min || value > max) {
    throw new Error("Save data contains an invalid position, facing, or tick fraction.");
  }
  return value;
}

export function demoSaveKey(characterId: string): string {
  if (!characterId || characterId.length > 128) throw new Error("A valid character ID is required.");
  return `mmorpg.offline-demo.v1.${characterId}`;
}

function validateDocument(value: unknown, characterId: string): DemoProgress {
  demoSaveKey(characterId);
  const saved = record(value, ["format", "version", "worldId", "characterId", "progress"]);
  if (saved.format !== FORMAT || saved.version !== VERSION || saved.worldId !== DEMO_WORLD_ID) {
    throw new Error("This save format, version, or world is not supported.");
  }
  if (saved.characterId !== characterId) throw new Error("This save belongs to another character.");
  const progress = record(saved.progress, ["position", "facing", "waystoneActive", "appearance", "tick", "tickFraction"]);
  const position = record(progress.position, ["x", "z"]);
  const appearance = record(progress.appearance, ["hat"]);
  if (!isHatStyle(appearance.hat) || typeof progress.waystoneActive !== "boolean") {
    throw new Error("Save appearance or waystone progress is invalid.");
  }
  if (typeof progress.tick !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(progress.tick)) {
    throw new Error("Save tick must be a canonical unsigned decimal string.");
  }
  const tick = BigInt(progress.tick);
  if (tick > MAX_DEMO_TICK) throw new Error("Save tick exceeds the supported range.");
  const tickFraction = numberInRange(progress.tickFraction, 0, 1);
  if (tickFraction === 1) throw new Error("Save tick fraction must be less than one.");
  return {
    position: {
      x: numberInRange(position.x, -DEMO_WORLD_HALF_EXTENT, DEMO_WORLD_HALF_EXTENT),
      z: numberInRange(position.z, -DEMO_WORLD_HALF_EXTENT, DEMO_WORLD_HALF_EXTENT),
    },
    facing: numberInRange(progress.facing, -Math.PI, Math.PI),
    waystoneActive: progress.waystoneActive,
    appearance: { hat: appearance.hat },
    tick,
    tickFraction,
  };
}

export function decodeDemoSave(raw: string, characterId: string): DemoProgress {
  // Reject oversized strings before allocating a UTF-8 copy or parsing JSON.
  if (raw.length > MAX_DEMO_SAVE_BYTES || new TextEncoder().encode(raw).byteLength > MAX_DEMO_SAVE_BYTES) {
    throw new Error("Save file is too large (maximum 8 KiB).");
  }
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new Error("Save file is not valid JSON.");
  }
  return validateDocument(value, characterId);
}

export function encodeDemoSave(progress: DemoProgress, characterId: string): string {
  if (typeof progress.tick !== "bigint") throw new Error("Demo tick must be a bigint.");
  const saved = {
    format: FORMAT,
    version: VERSION,
    worldId: DEMO_WORLD_ID,
    characterId,
    progress: {
      position: { ...progress.position },
      facing: progress.facing,
      waystoneActive: progress.waystoneActive,
      appearance: { ...progress.appearance },
      tick: progress.tick.toString(),
      tickFraction: progress.tickFraction,
    },
  };
  validateDocument(saved, characterId);
  const raw = JSON.stringify(saved);
  if (new TextEncoder().encode(raw).byteLength > MAX_DEMO_SAVE_BYTES) {
    throw new Error("Save file is too large (maximum 8 KiB).");
  }
  return raw;
}

/** Commands run on demand, never in the movement/render loop. Storage access is lazy. */
export class DemoSaveController {
  #operation = 0;

  constructor(
    private readonly characterId: string,
    private readonly capture: () => DemoProgress,
    private readonly restore: (progress: DemoProgress) => void,
    private readonly storage: () => CharacterStorage,
  ) {}

  /** Query only: inspecting a checkpoint must not restore it or retire pending intent. */
  inspect(): DemoProgress | null {
    const raw = this.storage().getItem(demoSaveKey(this.characterId));
    return raw === null ? null : decodeDemoSave(raw, this.characterId);
  }

  /** Navigation/customization supersedes any file read still in flight. */
  cancelPending(): void {
    this.#operation += 1;
  }

  save(): void {
    this.#operation += 1;
    const raw = encodeDemoSave(this.capture(), this.characterId);
    // One replacement write: a failed setItem must not delete the previous slot.
    this.storage().setItem(demoSaveKey(this.characterId), raw);
  }

  load(): boolean {
    this.#operation += 1;
    const progress = this.inspect();
    if (progress === null) return false;
    this.restore(progress);
    return true;
  }

  export(): string {
    this.#operation += 1;
    return encodeDemoSave(this.capture(), this.characterId);
  }

  async import(file: DemoSaveFile): Promise<boolean> {
    const operation = ++this.#operation;
    try {
      if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_DEMO_SAVE_BYTES) {
        throw new Error("Save file is too large (maximum 8 KiB).");
      }
      const raw = await file.text();
      if (operation !== this.#operation) return false;
      const progress = decodeDemoSave(raw, this.characterId);
      this.restore(progress);
      // Import changes the session only. Saving to the local slot is a separate explicit command.
      return true;
    } catch (error) {
      if (operation !== this.#operation) return false;
      throw error;
    }
  }
}
