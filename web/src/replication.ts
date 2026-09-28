/** Player-visible protocol v3 only. Canonical recovery state never enters rendering. */
export type Vector3 = readonly [number, number, number];
/** Wire kind 1. Creature (2) and NPC (3) codes are reserved and rejected until they exist. */
export type EntityKind = "player";
export type EntityState = {
  kind: EntityKind;
  entityId: number;
  position: Vector3;
  /** Presentation velocity in units per tick, saturated to the i16 wire range. */
  velocity: Vector3;
  /** u16 yaw: 65 536 steps per turn, 0 faces +Z, increasing turns toward +X. */
  facing: number;
};
export type ZoneSnapshot = {
  zoneId: number;
  tick: bigint;
  contentRevision: bigint;
  acknowledgedSequence: number;
  /** The player this projection is addressed to ("self"). */
  viewerId: number;
  entities: readonly EntityState[];
};

export const TICK_HZ = 30;
export const UNITS_PER_METRE = 100;
const YAW_STEPS = 65_536;
const WIRE_VERSION = 3;
const SCHEMA_VERSION = 3;
const PLAYER_SCOPE = 2;
const PLAYER_KIND_CODE = 1;
const MAX_ENTITIES = 512;
const HEADER_BYTES = 34;
const ENTITY_BYTES = 25;
const BUFFER_CAPACITY = 32;

export function decodeSnapshot(payload: Uint8Array): ZoneSnapshot {
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (view.byteLength < HEADER_BYTES) throw new Error("Truncated snapshot");
  if (view.getUint8(0) !== WIRE_VERSION || view.getUint16(2) !== SCHEMA_VERSION) {
    throw new Error("Unsupported snapshot version");
  }
  if (view.getUint8(1) !== PLAYER_SCOPE) throw new Error("Expected player-visible snapshot");
  const count = view.getUint16(32);
  if (count > MAX_ENTITIES || view.byteLength !== HEADER_BYTES + count * ENTITY_BYTES) {
    throw new Error("Invalid snapshot length or entity count");
  }
  const entities: EntityState[] = [];
  const identities = new Set<string>();
  for (let offset = HEADER_BYTES; offset < view.byteLength; offset += ENTITY_BYTES) {
    if (view.getUint8(offset) !== PLAYER_KIND_CODE) throw new Error("Unknown entity kind");
    const kind: EntityKind = "player";
    const entityId = view.getUint32(offset + 1);
    const identity = `${kind}:${entityId}`;
    if (identities.has(identity)) throw new Error("Duplicate entity identity");
    identities.add(identity);
    entities.push({
      kind,
      entityId,
      position: [view.getInt32(offset + 5), view.getInt32(offset + 9), view.getInt32(offset + 13)],
      velocity: [view.getInt16(offset + 17), view.getInt16(offset + 19), view.getInt16(offset + 21)],
      facing: view.getUint16(offset + 23),
    });
  }
  return {
    zoneId: view.getUint32(4),
    tick: view.getBigUint64(8),
    contentRevision: view.getBigUint64(16),
    acknowledgedSequence: view.getUint32(24),
    viewerId: view.getUint32(28),
    entities,
  };
}

/** Nearest u16 yaw for a presentation angle in radians (0 faces +Z, turning toward +X). */
export function yawFromRadians(radians: number): number {
  const turns = radians / (2 * Math.PI);
  return Math.round((turns - Math.floor(turns)) * YAW_STEPS) % YAW_STEPS;
}

/** Interpolates yaw along the shorter arc; the result stays in [0, 65 536). */
function interpolateFacing(from: number, to: number, alpha: number): number {
  const half = YAW_STEPS / 2;
  const delta = ((to - from + YAW_STEPS + half) % YAW_STEPS) - half;
  const facing = (from + delta * alpha) % YAW_STEPS;
  return facing < 0 ? facing + YAW_STEPS : facing;
}

/** Bounded presentation history. It never extrapolates gameplay or runs physics. */
export class SnapshotBuffer {
  #snapshots: ZoneSnapshot[] = [];

  /** Call on reconnect, handoff, or an authority-epoch change before accepting a new stream. */
  reset(): void {
    this.#snapshots = [];
  }

  push(snapshot: ZoneSnapshot): boolean {
    const latest = this.#snapshots.at(-1);
    if (latest) {
      if (
        snapshot.zoneId !== latest.zoneId ||
        snapshot.contentRevision !== latest.contentRevision ||
        snapshot.viewerId !== latest.viewerId
      ) {
        throw new Error("Snapshot stream changed without reset");
      }
      if (snapshot.tick <= latest.tick) return false;
      // After a long stall, snap to current state instead of interpolating a stale journey.
      if (snapshot.tick - latest.tick > BigInt(TICK_HZ * 30)) this.reset();
    }
    this.#snapshots.push(snapshot);
    if (this.#snapshots.length > BUFFER_CAPACITY) this.#snapshots.shift();
    return true;
  }

  sample(tick: bigint, fraction = 0): readonly EntityState[] {
    if (!Number.isFinite(fraction) || fraction < 0 || fraction >= 1) {
      throw new Error("Render tick fraction must be in [0, 1)");
    }
    let before = this.#snapshots[0];
    if (!before) return [];
    if (tick < before.tick) return before.entities;
    for (const after of this.#snapshots.slice(1)) {
      if (tick < after.tick) {
        const alpha = (Number(tick - before.tick) + fraction) / Number(after.tick - before.tick);
        const identity = (entity: EntityState) => `${entity.kind}:${entity.entityId}`;
        const nextEntities = new Map(after.entities.map((entity) => [identity(entity), entity]));
        return before.entities.map((entity) => {
          const next = nextEntities.get(identity(entity));
          // Appearance/disappearance happens at the authoritative sample tick.
          if (!next) return entity;
          const interpolate = (axis: 0 | 1 | 2) =>
            entity.position[axis] + (next.position[axis] - entity.position[axis]) * alpha;
          return {
            ...entity,
            position: [interpolate(0), interpolate(1), interpolate(2)],
            facing: interpolateFacing(entity.facing, next.facing, alpha),
          };
        });
      }
      before = after;
    }
    return before.entities;
  }
}
