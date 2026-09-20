/** Player-visible protocol v2 only. Canonical recovery state never enters rendering. */
export type Vector3 = readonly [number, number, number];
export type PlayerState = {
  playerId: number;
  position: Vector3;
  velocity: Vector3;
};
export type ZoneSnapshot = {
  zoneId: number;
  tick: bigint;
  contentRevision: bigint;
  acknowledgedSequence: number;
  players: readonly PlayerState[];
};

export const TICK_HZ = 30;
export const UNITS_PER_METRE = 100;
const MAX_PLAYERS = 512;
const HEADER_BYTES = 30;
const PLAYER_BYTES = 28;
const BUFFER_CAPACITY = 32;

export function decodeSnapshot(payload: Uint8Array): ZoneSnapshot {
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (view.byteLength < HEADER_BYTES) throw new Error("Truncated snapshot");
  if (view.getUint8(0) !== 2 || view.getUint16(2) !== 2) {
    throw new Error("Unsupported snapshot version");
  }
  if (view.getUint8(1) !== 2) throw new Error("Expected player-visible snapshot");
  const count = view.getUint16(16);
  if (count > MAX_PLAYERS || view.byteLength !== HEADER_BYTES + count * PLAYER_BYTES) {
    throw new Error("Invalid snapshot length or player count");
  }
  const players: PlayerState[] = [];
  const ids = new Set<number>();
  for (let offset = HEADER_BYTES; offset < view.byteLength; offset += PLAYER_BYTES) {
    const playerId = view.getUint32(offset);
    if (ids.has(playerId)) throw new Error("Duplicate player identity");
    ids.add(playerId);
    players.push({
      playerId,
      position: [view.getInt32(offset + 4), view.getInt32(offset + 8), view.getInt32(offset + 12)],
      velocity: [view.getInt32(offset + 16), view.getInt32(offset + 20), view.getInt32(offset + 24)],
    });
  }
  return {
    zoneId: view.getUint32(4),
    tick: view.getBigUint64(8),
    contentRevision: view.getBigUint64(18),
    acknowledgedSequence: view.getUint32(26),
    players,
  };
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
      if (snapshot.zoneId !== latest.zoneId || snapshot.contentRevision !== latest.contentRevision) {
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

  sample(tick: bigint, fraction = 0): readonly PlayerState[] {
    if (!Number.isFinite(fraction) || fraction < 0 || fraction >= 1) {
      throw new Error("Render tick fraction must be in [0, 1)");
    }
    let before = this.#snapshots[0];
    if (!before) return [];
    if (tick < before.tick) return before.players;
    for (const after of this.#snapshots.slice(1)) {
      if (tick < after.tick) {
        const alpha = (Number(tick - before.tick) + fraction) / Number(after.tick - before.tick);
        const nextPlayers = new Map(after.players.map((player) => [player.playerId, player]));
        return before.players.map((player) => {
          const next = nextPlayers.get(player.playerId);
          // Appearance/disappearance happens at the authoritative sample tick.
          if (!next) return player;
          const interpolate = (axis: 0 | 1 | 2) =>
            player.position[axis] + (next.position[axis] - player.position[axis]) * alpha;
          return { ...player, position: [interpolate(0), interpolate(1), interpolate(2)] };
        });
      }
      before = after;
    }
    return before.players;
  }
}
