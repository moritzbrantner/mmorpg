import { describe, expect, test } from "bun:test";
import {
  POSE_LIMITS,
  RUN_SPEED,
  UnitAnimator,
  poseFor,
  type LocomotionState,
  type Pose,
  type Stance,
} from "../src/world/character-animation";

const STANCES: Stance[] = ["sword-and-shield", "bow", "staff"];

function state(patch: Partial<LocomotionState> = {}): LocomotionState {
  return {
    forward: 0,
    right: 0,
    vertical: 0,
    moveWeight: 0,
    airWeight: 0,
    turnRate: 0,
    stridePhase: 0,
    shufflePhase: 0,
    time: 0,
    ...patch,
  };
}

function within(pose: Pose): void {
  const check = (value: number, [low, high]: readonly [number, number], name: string) => {
    if (!Number.isFinite(value) || value < low - 1e-12 || value > high + 1e-12) {
      throw new Error(`${name} = ${value} outside [${low}, ${high}]`);
    }
  };
  check(pose.bob, POSE_LIMITS.bob, "bob");
  check(pose.lean, POSE_LIMITS.lean, "lean");
  check(pose.twist, POSE_LIMITS.twist, "twist");
  check(pose.hipYaw, POSE_LIMITS.hipYaw, "hipYaw");
  check(pose.breathe, POSE_LIMITS.breathe, "breathe");
  check(pose.headPitch, POSE_LIMITS.headPitch, "headPitch");
  check(pose.cape, POSE_LIMITS.cape, "cape");
  for (const leg of [pose.leftLeg, pose.rightLeg]) {
    check(leg.swing, POSE_LIMITS.legSwing, "legSwing");
    check(leg.spread, POSE_LIMITS.legSpread, "legSpread");
    check(leg.knee, POSE_LIMITS.knee, "knee");
    check(leg.foot, POSE_LIMITS.foot, "foot");
  }
  for (const arm of [pose.leftArm, pose.rightArm]) {
    check(arm.swing, POSE_LIMITS.armSwing, "armSwing");
    check(arm.spread, POSE_LIMITS.armSpread, "armSpread");
    check(arm.elbow, POSE_LIMITS.elbow, "elbow");
  }
}

/** A tiny seeded generator so the sweep is repeatable. */
function* samples(count: number): Generator<LocomotionState> {
  let seed = 12_345;
  const next = () => {
    seed = (Math.imul(seed, 1_103_515_245) + 12_345) >>> 0;
    return seed / 4_294_967_296;
  };
  const wild = [Number.NaN, Number.POSITIVE_INFINITY, -1e9, 1e9, -0];
  for (let index = 0; index < count; index += 1) {
    const pick = (scale: number) => (next() < 0.05 ? wild[Math.floor(next() * wild.length)]! : (next() * 2 - 1) * scale);
    yield state({
      forward: pick(12),
      right: pick(12),
      vertical: pick(10),
      moveWeight: pick(1.5),
      airWeight: pick(1.5),
      turnRate: pick(20),
      stridePhase: pick(400),
      shufflePhase: pick(400),
      time: pick(5_000),
    });
  }
}

describe("procedural pose", () => {
  test("is pure and bounded for any input, including non-finite values", () => {
    for (const input of samples(3_000)) {
      const copy = { ...input };
      for (const stance of STANCES) {
        const pose = poseFor(input, stance);
        within(pose);
        expect(poseFor(input, stance)).toEqual(pose);
      }
      expect(input).toEqual(copy);
    }
  });

  test("running swings the legs and arms in opposition; standing does not", () => {
    const idle = poseFor(state(), "bow");
    expect(Math.abs(idle.leftLeg.swing)).toBeLessThan(1e-9);
    const run = poseFor(state({ forward: RUN_SPEED, moveWeight: 1, stridePhase: Math.PI / 2 }), "bow");
    expect(run.leftLeg.swing).toBeGreaterThan(0.6);
    expect(run.rightLeg.swing).toBeLessThan(-0.6);
    expect(run.leftArm.swing).toBeLessThan(0);
    expect(run.rightArm.swing).toBeGreaterThan(0);
    expect(run.lean).toBeGreaterThan(idle.lean);
    expect(run.cape).toBeGreaterThan(idle.cape);
  });

  test("backpedalling reverses the stride and strafing sidesteps", () => {
    const forward = poseFor(state({ forward: RUN_SPEED, moveWeight: 1, stridePhase: Math.PI / 2 }), "staff");
    const back = poseFor(state({ forward: -3.9, moveWeight: 1, stridePhase: Math.PI / 2 }), "staff");
    expect(Math.sign(back.leftLeg.swing)).toBe(-Math.sign(forward.leftLeg.swing));
    expect(back.lean).toBeLessThan(0);
    const strafe = poseFor(state({ right: RUN_SPEED, moveWeight: 1, stridePhase: Math.PI / 2 }), "staff");
    expect(Math.abs(strafe.leftLeg.spread)).toBeGreaterThan(0.1);
    expect(Math.abs(strafe.leftLeg.swing)).toBeLessThan(Math.abs(forward.leftLeg.swing));
    const diagonal = poseFor(state({ forward: 4.4, right: 4.4, moveWeight: 1 }), "staff");
    expect(diagonal.hipYaw).toBeGreaterThan(0.3);
  });

  test("jumping tucks the legs and turning in place shuffles the feet", () => {
    const rising = poseFor(state({ vertical: 4, airWeight: 1 }), "sword-and-shield");
    const grounded = poseFor(state(), "sword-and-shield");
    expect(rising.leftLeg.knee).toBeGreaterThan(grounded.leftLeg.knee + 0.8);
    expect(rising.leftArm.spread).toBeGreaterThan(grounded.leftArm.spread);
    const shuffle = poseFor(state({ turnRate: 4, shufflePhase: Math.PI / 2 }), "sword-and-shield");
    expect(shuffle.leftLeg.knee).toBeGreaterThan(grounded.leftLeg.knee + 0.2);
  });
});

describe("unit animator", () => {
  const sample = (x: number, facing = 0, velocity: [number, number, number] = [0, 0, 0]) => ({ x, z: 0, velocity, facing });

  test("advances the stride by distance travelled, not by time", () => {
    const walker = new UnitAnimator();
    walker.update(sample(0), 1 / 60, false);
    const moved = walker.update(sample(1.3, Math.PI / 2, [RUN_SPEED, 0, 0]), 1 / 60, false);
    expect(moved.stridePhase).toBeCloseTo(Math.PI, 9);
    expect(moved.forward).toBeCloseTo(RUN_SPEED, 9);
    const waited = walker.update(sample(1.3, Math.PI / 2), 5, false);
    expect(waited.stridePhase).toBeCloseTo(Math.PI, 9);
    // A teleport (re-entry, respawn) does not spin the stride.
    expect(walker.update(sample(400, Math.PI / 2), 1 / 60, false).stridePhase).toBeCloseTo(Math.PI, 9);
  });

  test("blends in over a short ramp, or at once with reduced motion", () => {
    const smooth = new UnitAnimator();
    const first = smooth.update(sample(0, 0, [0, 0, RUN_SPEED]), 0.05, false);
    expect(first.moveWeight).toBeGreaterThan(0);
    expect(first.moveWeight).toBeLessThan(1);
    const instant = new UnitAnimator();
    expect(instant.update(sample(0, 0, [0, 0, RUN_SPEED]), 0.05, true).moveWeight).toBe(1);
    expect(instant.update(sample(0, 0), 0.05, true).moveWeight).toBe(0);
  });

  test("with reduced motion a standing unit repeats the same pose every frame", () => {
    const animator = new UnitAnimator();
    const poses = [0.016, 0.02, 0.5].map((dt) => poseFor(animator.update(sample(3), dt, true), "bow"));
    expect(poses[1]).toEqual(poses[0]!);
    expect(poses[2]).toEqual(poses[0]!);
    const breathing = new UnitAnimator();
    const a = poseFor(breathing.update(sample(3), 0.5, false), "bow");
    const b = poseFor(breathing.update(sample(3), 0.5, false), "bow");
    expect(b.breathe).not.toBe(a.breathe);
  });

  test("turning measures the facing change along the shorter arc", () => {
    const animator = new UnitAnimator();
    animator.update(sample(0, 2 * Math.PI - 0.1), 0.1, true);
    const turned = animator.update(sample(0, 0.1), 0.1, true);
    expect(turned.turnRate).toBeCloseTo(2, 9);
    expect(turned.shufflePhase).toBeGreaterThan(0);
  });
});
