import type { WorldCommand } from "../command-wire";
import type { EntityState, ZoneSnapshot } from "../replication";

/**
 * Where the presentation layer gets its world. A source admits one local
 * player, carries that player's intent to an authoritative zone, and hands
 * back only decoded player-scoped projections. `LocalZoneSource` runs the
 * shared zone simulation in WASM; an online WebTransport source (issue #29)
 * implements the same contract, so presentation cannot tell them apart.
 */
export type WorldSource = {
  /** Enters the world as a new player and returns its ID. Throws while joined. */
  join(): number;
  /** Leaves the world; the player's unit is removed. Does nothing when not joined. */
  leave(): void;
  /** Sends intent under the next strictly increasing sequence. Throws when not joined. */
  sendCommand(command: WorldCommand): void;
  /** Lets the source progress by elapsed wall-clock seconds. */
  advance(deltaSeconds: number): void;
  /** The newest decoded projection addressed to the joined player, if any. */
  latestProjection(): ZoneSnapshot | null;
  /** Interpolated entities to draw now; empty before the first projection. */
  sample(): readonly EntityState[];
};
