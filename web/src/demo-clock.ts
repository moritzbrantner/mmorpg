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
