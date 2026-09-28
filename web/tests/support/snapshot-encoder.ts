import type { ZoneSnapshot } from "../../src/replication";

/** Test-only player-visible snapshot v3 encoder (docs/PROTOCOL.md); Rust owns the real one. */
export function encodeTestSnapshot(snapshot: ZoneSnapshot): Uint8Array {
  const bytes = new Uint8Array(34 + 25 * snapshot.entities.length);
  const view = new DataView(bytes.buffer);
  view.setUint8(0, 3);
  view.setUint8(1, 2);
  view.setUint16(2, 3);
  view.setUint32(4, snapshot.zoneId);
  view.setBigUint64(8, snapshot.tick);
  view.setBigUint64(16, snapshot.contentRevision);
  view.setUint32(24, snapshot.acknowledgedSequence);
  view.setUint32(28, snapshot.viewerId);
  view.setUint16(32, snapshot.entities.length);
  snapshot.entities.forEach((entity, index) => {
    const offset = 34 + 25 * index;
    view.setUint8(offset, 1);
    view.setUint32(offset + 1, entity.entityId);
    entity.position.forEach((component, axis) => view.setInt32(offset + 5 + 4 * axis, component));
    entity.velocity.forEach((component, axis) => view.setInt16(offset + 17 + 2 * axis, component));
    view.setUint16(offset + 23, entity.facing);
  });
  return bytes;
}
