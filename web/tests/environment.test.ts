import { describe, expect, test } from "bun:test";
import { DAY_SECONDS, ENVIRONMENT, hazeMix, mixColor, quantizeColor, shadeColor, skyAt } from "../src/world/environment";

const HEX = /^#[0-9a-f]{6}$/;

describe("environment style", () => {
  test("colour helpers stay valid colours", () => {
    expect(mixColor("#000000", "#ffffff", 0.5)).toBe("#808080");
    expect(mixColor("#102030", "#ffffff", Number.NaN)).toBe("#102030");
    expect(mixColor("#102030", "#ffffff", 7)).toBe("#ffffff");
    expect(shadeColor("#ff8000", 2)).toBe("#ffff00");
    expect(quantizeColor("#0b1f3d", 8)).toBe("#082040");
  });

  test("haze grows with distance band by band", () => {
    let previous = -1;
    for (const distance of [0, 100, 240, 320, 400, 500, 900]) {
      const mix = hazeMix(ENVIRONMENT, distance);
      expect(mix).toBeGreaterThanOrEqual(previous);
      previous = mix;
    }
    expect(hazeMix(ENVIRONMENT, 0)).toBe(0);
  });

  test("the sky cycle is periodic, subtle and starts at the style's midday", () => {
    expect(skyAt(ENVIRONMENT, 0)).toEqual(ENVIRONMENT.sky);
    expect(skyAt(ENVIRONMENT, 1)).toEqual(ENVIRONMENT.sky);
    expect(skyAt(ENVIRONMENT, 0.25)).toEqual(skyAt(ENVIRONMENT, 1.25));
    for (const fraction of [0.1, 0.5, 0.9, Number.NaN]) {
      const stops = skyAt(ENVIRONMENT, fraction);
      for (const color of Object.values(stops)) {
        expect(color).toMatch(HEX);
      }
      expect(stops.ground).toBe(ENVIRONMENT.sky.ground);
    }
    expect(skyAt(ENVIRONMENT, 0.5).zenith).not.toBe(ENVIRONMENT.sky.zenith);
    expect(DAY_SECONDS).toBe(1_200);
  });
});
