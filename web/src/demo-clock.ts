/** Offline presentation clock; this never changes server-authoritative tick semantics. */
export const MAX_DEMO_TICK = 18446744073709551615n;

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
