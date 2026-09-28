import { describe, expect, test } from "bun:test";
import { FrameRate } from "../src/world/debug-overlay";

describe("frame rate", () => {
  test("measures frames per second over a sliding window", () => {
    const rate = new FrameRate(1_000);
    expect(rate.fps).toBe(0);
    for (let frame = 0; frame <= 120; frame += 1) {
      rate.sample(frame * 20);
    }
    expect(rate.fps).toBeCloseTo(50, 6);
    expect(rate.frameMs).toBeCloseTo(20, 6);
    rate.reset();
    expect(rate.fps).toBe(0);
  });
});
