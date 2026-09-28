import type { ZoneSnapshot } from "../../src/replication";

const HEADER_BYTES = 34;
const ENTITY_BYTES = 16;

/**
 * Test-only player-visible snapshot v4 encoder (docs/PROTOCOL.md); Rust owns the real one.
 * Like it, positions must fit i16 and velocities saturate to i8.
 */
export function encodeTestSnapshot(snapshot: ZoneSnapshot): Uint8Array {
  const bytes = new Uint8Array(HEADER_BYTES + ENTITY_BYTES * snapshot.entities.length);
  const view = new DataView(bytes.buffer);
  view.setUint8(0, 4);
  view.setUint8(1, 2);
  view.setUint16(2, 4);
  view.setUint32(4, snapshot.zoneId);
  view.setBigUint64(8, snapshot.tick);
  view.setBigUint64(16, snapshot.contentRevision);
  view.setUint32(24, snapshot.acknowledgedSequence);
  view.setUint32(28, snapshot.viewerId);
  view.setUint16(32, snapshot.entities.length);
  snapshot.entities.forEach((entity, index) => {
    const offset = HEADER_BYTES + ENTITY_BYTES * index;
    view.setUint8(offset, 1);
    view.setUint32(offset + 1, entity.entityId);
    entity.position.forEach((component, axis) => {
      if (component < -32_768 || component > 32_767) throw new Error("position does not fit i16");
      view.setInt16(offset + 5 + 2 * axis, component);
    });
    entity.velocity.forEach((component, axis) =>
      view.setInt8(offset + 11 + axis, Math.max(-128, Math.min(127, component))));
    view.setUint16(offset + 14, entity.facing);
  });
  return bytes;
}
