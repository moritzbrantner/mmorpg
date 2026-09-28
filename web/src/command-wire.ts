/**
 * Command wire version 2, mirrored from `mmorpg-protocol` (docs/PROTOCOL.md).
 * `fixtures/protocol/commands-v2.hex` holds both encoders to the same bytes.
 * The session supplies player identity and sequence separately.
 */
export type Axis = -1 | 0 | 1;

export type WorldCommand =
  | { kind: "move"; forward: Axis; strafe: Axis; facing: number }
  | { kind: "jump" };

const COMMAND_WIRE_VERSION = 2;
const MOVE_TAG = 1;
const JUMP_TAG = 2;
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
    case "jump":
      return Uint8Array.of(COMMAND_WIRE_VERSION, JUMP_TAG);
  }
}
