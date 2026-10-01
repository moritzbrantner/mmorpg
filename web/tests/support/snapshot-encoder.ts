import { entityKindCode, type EntityRef } from "../../src/entity-ref";
import type { ZoneEvent, ZoneSnapshot } from "../../src/replication";

const FIXED_BYTES = 72;
const EVENT_BYTES = 14;
const ENTITY_BYTES = 21;
const ERROR_CODES = [
  "no-target", "out-of-range", "target-dead", "not-attackable", "you-are-dead", "not-dead", "invalid-target", "too-many-intents", "invalid-inventory-move", "inventory-full",
];

function flagByte(flags: readonly boolean[]): number {
  return flags.reduce((byte, set, bit) => byte | (set ? 1 << bit : 0), 0);
}

/**
 * Test-only player-visible snapshot v7 encoder (docs/PROTOCOL.md); Rust owns the real one.
 * Like it, positions must fit i16 and velocities saturate to i8.
 */
export function encodeTestSnapshot(snapshot: ZoneSnapshot): Uint8Array {
  const size = FIXED_BYTES + (snapshot.inventory === null ? 0 : 64) + EVENT_BYTES * snapshot.events.length + ENTITY_BYTES * snapshot.entities.length;
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  let offset = 0;
  const u8 = (value: number) => { view.setUint8(offset, value); offset += 1; };
  const u16 = (value: number) => { view.setUint16(offset, value); offset += 2; };
  const u32 = (value: number) => { view.setUint32(offset, value); offset += 4; };
  const u64 = (value: bigint) => { view.setBigUint64(offset, value); offset += 8; };
  const entity = (value: EntityRef | null) => {
    u8(value === null ? 0 : entityKindCode(value.kind));
    u32(value?.id ?? 0);
  };
  u8(7);
  u8(2);
  u16(7);
  u32(snapshot.zoneId);
  u64(snapshot.tick);
  u64(snapshot.contentRevision);
  u32(snapshot.acknowledgedSequence);
  u32(snapshot.viewerId);
  const viewer = snapshot.viewer;
  u32(viewer.health);
  u32(viewer.maxHealth);
  u8(viewer.level);
  u32(viewer.experience);
  u32(viewer.experienceToNextLevel);
  u8(flagByte([viewer.dead, viewer.inCombat, viewer.autoAttacking]));
  entity(viewer.target);
  entity(snapshot.targetOfTarget);
  u64(snapshot.inventoryRevision);
  u8(snapshot.inventory === null ? 0 : 1);
  if (snapshot.inventory !== null) {
    if (snapshot.inventory.length !== 16) throw new Error("Inventory must have sixteen slots");
    for (const slot of snapshot.inventory) {
      u16(slot?.itemId ?? 0);
      u16(slot?.quantity ?? 0);
    }
  }
  u8(snapshot.events.length);
  for (const event of snapshot.events) {
    encodeEvent(event, u8, u16, entity);
  }
  u16(snapshot.entities.length);
  for (const record of snapshot.entities) {
    u8(entityKindCode(record.kind));
    u32(record.entityId);
    u16(record.appearance);
    for (const component of record.position) {
      if (component < -32_768 || component > 32_767) throw new Error("position does not fit i16");
      view.setInt16(offset, component);
      offset += 2;
    }
    for (const component of record.velocity) {
      view.setInt8(offset, Math.max(-128, Math.min(127, component)));
      offset += 1;
    }
    u16(record.facing);
    u8(record.level);
    u8(record.healthPercent);
    const flags = record.flags;
    u8(flagByte([flags.dead, flags.inCombat, flags.hostile, flags.attackable, flags.tappedByOther, flags.evading, flags.targetsViewer]));
  }
  return bytes;
}

function encodeEvent(
  event: ZoneEvent,
  u8: (value: number) => void,
  u16: (value: number) => void,
  entity: (value: EntityRef | null) => void,
): void {
  switch (event.kind) {
    case "damage-dealt":
    case "damage-taken":
      u8(event.kind === "damage-dealt" ? 1 : 2);
      u8(event.critical ? 1 : 0);
      entity(event.source);
      entity(event.target);
      u16(event.amount);
      return;
    case "miss":
    case "evade":
      u8(event.kind === "miss" ? 3 : 5);
      u8(0);
      entity(event.source);
      entity(event.target);
      u16(0);
      return;
    case "died":
      u8(4);
      u8(0);
      entity(event.killer);
      entity(event.entity);
      u16(0);
      return;
    case "error":
      u8(6);
      u8(0);
      entity(null);
      entity(event.target);
      u16(ERROR_CODES.indexOf(event.code) + 1);
      return;
  }
}
