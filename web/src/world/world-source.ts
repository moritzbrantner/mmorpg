import type { WorldCommand } from "../command-wire";
import type { EntityState, ZoneSnapshot } from "../replication";

/** The class and sex a new player chooses on entry; the zone takes the choice once. */
export type JoinCharacter = { classId: "warden" | "ranger" | "arcanist"; sex: "female" | "male" };

/** The default entry character: a male Warden, like the native client. */
export const DEFAULT_JOIN_CHARACTER: JoinCharacter = { classId: "warden", sex: "male" };

/** Wire values of a class choice (0 Warden, 1 Ranger, 2 Arcanist; 0 female, 1 male). */
export function classChoiceCodes(character: JoinCharacter): { classId: number; sex: number } {
  return {
    classId: ["warden", "ranger", "arcanist"].indexOf(character.classId),
    sex: character.sex === "female" ? 0 : 1,
  };
}

/**
 * Where the presentation layer gets its world. A source admits one local
 * player, carries that player's intent to an authoritative zone, and hands
 * back only decoded player-scoped projections. `LocalZoneSource` runs the
 * shared zone simulation in WASM; an online WebTransport source (issue #29)
 * implements the same contract, so presentation cannot tell them apart.
 */
export type WorldSource = {
  /**
   * Enters the world as a new player and returns its ID. Throws while joined.
   * A join that throws leaves the source unjoined, so entry can be retried. The new player
   * chooses `character`'s class and sex before any other command.
   */
  join(character?: JoinCharacter): number;
  /** Leaves the world; the player's unit is removed. Does nothing when not joined. */
  leave(): void;
  /** Sends intent under the next strictly increasing sequence. Throws when not joined. */
  sendCommand(command: WorldCommand): void;
  /** Progresses by elapsed seconds and returns received projections in tick order.
   * Local catch-up is bounded to four ticks; intermediate sheets/events are retained. */
  advance(deltaSeconds: number): readonly ZoneSnapshot[];
  /** The newest decoded projection addressed to the joined player, if any. */
  latestProjection(): ZoneSnapshot | null;
  /** Interpolated entities to draw now; empty before the first projection. */
  sample(): readonly EntityState[];
};
