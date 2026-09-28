/** Presentation clock helpers; they never change authoritative tick semantics. */
import { TICK_HZ } from "./replication";

/** Offline presentation clock; this never changes server-authoritative tick semantics. */
export const MAX_DEMO_TICK = 18446744073709551615n;

/** A long stall runs at most this many catch-up ticks; the rest is dropped. */
export const MAX_CATCH_UP_TICKS = 4;

export function advanceDemoTick(tick: bigint, presentation: { reset(): void }): bigint {
  if (tick === MAX_DEMO_TICK) {
    // A restarted clock must not interpolate against the previous tick epoch.
    presentation.reset();
    return 0n;
  }
  return tick + 1n;
}

export function frameDeltaSeconds(now: number, previous: number): number {
  if (!Number.isFinite(now) || !Number.isFinite(previous)) {
    throw new Error("Frame timestamps must be finite.");
  }
  return Math.max(0, Math.min((now - previous) / 1000, 0.05));
}

/** Bounded accumulator that turns elapsed wall-clock time into whole 30 Hz ticks. */
export class FixedTickClock {
  #accumulated = 0;

  /** Elapsed fraction of the next tick, in [0, 1). */
  get fraction(): number {
    return this.#accumulated;
  }

  reset(): void {
    this.#accumulated = 0;
  }

  /** Adds elapsed seconds and returns how many ticks are due now, at most `MAX_CATCH_UP_TICKS`. */
  advance(deltaSeconds: number): number {
    if (!Number.isFinite(deltaSeconds) || deltaSeconds < 0) {
      throw new Error("Elapsed time must be finite and non-negative.");
    }
    const total = this.#accumulated + deltaSeconds * TICK_HZ;
    const due = Math.floor(total);
    this.#accumulated = total - due;
    return Math.min(due, MAX_CATCH_UP_TICKS);
  }
}
