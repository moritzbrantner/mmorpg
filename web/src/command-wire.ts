/**
 * Command wire version 3, mirrored from `mmorpg-protocol` (docs/PROTOCOL.md).
 * `fixtures/protocol/commands-v3.hex` holds both encoders to the same bytes.
 * The session supplies player identity and sequence separately.
 */
import { entityKindCode, isU32, type EntityRef } from "./entity-ref";

export type Axis = -1 | 0 | 1;

export type WorldCommand =
  | { kind: "move"; forward: Axis; strafe: Axis; facing: number }
  | { kind: "loot"; creatureId: number; diedAt: bigint }
  | { kind: "jump" }
  /** Selects a unit, or clears the selection with `null`. */
  | { kind: "select-target"; target: EntityRef | null }
  | { kind: "start-attack" }
  | { kind: "stop-attack" }
  | { kind: "release-spirit" }
  | { kind: "move-item"; source: number; destination: number; quantity: number };

const COMMAND_WIRE_VERSION = 3;
const MOVE_TAG = 1;
const JUMP_TAG = 2;
const SELECT_TARGET_TAG = 3;
const START_ATTACK_TAG = 4;
const STOP_ATTACK_TAG = 5;
const RELEASE_SPIRIT_TAG = 6;
const MOVE_ITEM_TAG = 7;
const LOOT_TAG = 8;
const YAW_STEPS = 65_536;

function isAxis(value: number): value is Axis {
  return value === -1 || value === 0 || value === 1;
}

export function encodeCommand(command: WorldCommand): Uint8Array {
  switch (command.kind) {
    case "move": {
      if (!isAxis(command.forward) || !isAxis(command.strafe)) {
        throw new Error("Movement components must be -1, 0 or 1.");
      }
      if (!Number.isInteger(command.facing) || command.facing < 0 || command.facing >= YAW_STEPS) {
        throw new Error("Facing must be a u16 yaw.");
      }
      const payload = new Uint8Array(6);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, MOVE_TAG);
      view.setInt8(2, command.forward);
      view.setInt8(3, command.strafe);
      view.setUint16(4, command.facing);
      return payload;
    }
    case "move-item": {
      if (![command.source, command.destination].every((slot) => Number.isInteger(slot) && slot >= 0 && slot <= 255)
        || !Number.isInteger(command.quantity) || command.quantity < 0 || command.quantity > 65_535) {
        throw new Error("Bag move fields must fit u8 slots and a u16 quantity.");
      }
      const payload = new Uint8Array(6);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, MOVE_ITEM_TAG);
      view.setUint8(2, command.source);
      view.setUint8(3, command.destination);
      view.setUint16(4, command.quantity);
      return payload;
    }
    case "loot": {
      if (!isU32(command.creatureId) || command.diedAt < 0n || command.diedAt > 0xffff_ffff_ffff_ffffn) {
        throw new Error("Loot claims require a u32 creature and u64 death tick.");
      }
      const payload = new Uint8Array(14);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, LOOT_TAG);
      view.setUint32(2, command.creatureId);
      view.setBigUint64(6, command.diedAt);
      return payload;
    }
    case "jump":
      return Uint8Array.of(COMMAND_WIRE_VERSION, JUMP_TAG);
    case "select-target": {
      const payload = new Uint8Array(7);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, SELECT_TARGET_TAG);
      if (command.target !== null) {
        if (!isU32(command.target.id)) {
          throw new Error("Target IDs must be u32.");
        }
        view.setUint8(2, entityKindCode(command.target.kind));
        view.setUint32(3, command.target.id);
      }
      return payload;
    }
    case "start-attack":
      return Uint8Array.of(COMMAND_WIRE_VERSION, START_ATTACK_TAG);
    case "stop-attack":
      return Uint8Array.of(COMMAND_WIRE_VERSION, STOP_ATTACK_TAG);
    case "release-spirit":
      return Uint8Array.of(COMMAND_WIRE_VERSION, RELEASE_SPIRIT_TAG);
    default: {
      const unsupported: never = command;
      throw new Error(`Unsupported command ${String(unsupported)}.`);
    }
  }
}
