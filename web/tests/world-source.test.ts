import { describe, expect, test } from "bun:test";
import { encodeCommand } from "../src/command-wire";
import { MAX_CATCH_UP_TICKS } from "../src/demo-clock";
import { LocalZoneSource, type LocalZoneHandle } from "../src/world/local-zone-source";
import { FACING_INTERVAL_MS, MovementOutbox, RESEND_INTERVAL_MS, type MovementInput } from "../src/world/movement-outbox";
import { MAX_DISTANCE_METRES, MIN_DISTANCE_METRES, ORBIT_RADIANS_PER_PIXEL, OrbitCamera, heldIntent } from "../src/world/orbit-camera";
import { FakeWorldSource } from "./support/fake-world-source";
import { encodeTestSnapshot } from "./support/snapshot-encoder";
import { worldSourceContract } from "./support/world-source-contract";

/** Records what `LocalZoneSource` asks of the WASM zone; returns well-formed projections. */
class RecordingZone implements LocalZoneHandle {
  readonly submitted: { player: number; sequence: number; bytes: string }[] = [];
  readonly left: number[] = [];
  ticks = 0;
  viewerOverride: number | null = null;
  #nextPlayer = 1;
  #players = new Set<number>();

  join(): number {
    const player = this.#nextPlayer;
    this.#nextPlayer += 1;
    this.#players.add(player);
    return player;
  }

  leave(player: number): boolean {
    this.left.push(player);
    return this.#players.delete(player);
  }

  submit(player: number, sequence: number, command: Uint8Array): boolean {
    this.submitted.push({ player, sequence, bytes: Buffer.from(command).toString("hex") });
    return true;
  }

  tick(): bigint {
    this.ticks += 1;
    return BigInt(this.ticks);
  }

  projection(player: number): Uint8Array {
    if (!this.#players.has(player)) throw new Error("unknown player");
    const viewer = this.viewerOverride ?? player;
    return encodeTestSnapshot({
      zoneId: 1, tick: BigInt(this.ticks), contentRevision: 1n, acknowledgedSequence: 0, viewerId: viewer,
      entities: [{ kind: "player", entityId: viewer, position: [this.ticks * 21, 90, 0], velocity: [21, 0, 0], facing: 0 }],
    });
  }
}

worldSourceContract("FakeWorldSource", () => new FakeWorldSource());
worldSourceContract("LocalZoneSource over a recording zone", () => new LocalZoneSource(new RecordingZone()));

describe("LocalZoneSource", () => {
  test("sends encoded commands with strictly increasing sequences per player", () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(zone);
    const first = source.join();
    source.sendCommand({ kind: "move", forward: 1, strafe: -1, facing: 16_384 });
    source.sendCommand({ kind: "jump" });
    source.leave();
    const second = source.join();
    source.sendCommand({ kind: "jump" });
    expect(zone.submitted).toEqual([
      { player: first, sequence: 1, bytes: "020101ff4000" },
      { player: first, sequence: 2, bytes: Buffer.from(encodeCommand({ kind: "jump" })).toString("hex") },
      { player: second, sequence: 1, bytes: "0202" },
    ]);
    expect(zone.left).toEqual([first]);
  });

  test("runs fixed ticks from a bounded accumulator only while joined", () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(zone);
    source.advance(1);
    expect(zone.ticks).toBe(0);
    source.join();
    source.advance(0.5 / 30);
    expect(zone.ticks).toBe(0);
    source.advance(0.5 / 30 + 1e-9);
    expect(zone.ticks).toBe(1);
    source.advance(10);
    expect(zone.ticks).toBe(1 + MAX_CATCH_UP_TICKS);
    expect(source.latestProjection()?.tick).toBe(BigInt(1 + MAX_CATCH_UP_TICKS));
  });

  test("samples interpolate one tick behind the latest projection", () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(zone);
    source.join();
    source.advance(2 / 30 + 1e-9);
    expect(zone.ticks).toBe(2);
    expect(source.sample()[0]?.position[0]).toBeCloseTo(21, 3);
    source.advance(0.5 / 30);
    expect(source.sample()[0]?.position[0]).toBeCloseTo(31.5, 3);
  });

  test("a projection addressed to another player is rejected", () => {
    const zone = new RecordingZone();
    zone.viewerOverride = 99;
    expect(() => new LocalZoneSource(zone).join()).toThrow("another player");
  });
});

describe("movement outbox", () => {
  const input = (patch: Partial<MovementInput> = {}): MovementInput => ({ forward: 0, strafe: 0, facing: 0, jumps: 0, ...patch });

  test("sends current intent first, then only changes, and resends on the heartbeat", () => {
    const outbox = new MovementOutbox(input());
    expect(outbox.update(input(), 0)).toEqual([{ kind: "move", forward: 0, strafe: 0, facing: 0 }]);
    expect(outbox.update(input(), 10)).toEqual([]);
    expect(outbox.update(input({ forward: 1 }), 11)).toEqual([{ kind: "move", forward: 1, strafe: 0, facing: 0 }]);
    expect(outbox.update(input({ forward: 1 }), 11 + RESEND_INTERVAL_MS - 1)).toEqual([]);
    expect(outbox.update(input({ forward: 1 }), 11 + RESEND_INTERVAL_MS)).toEqual([{ kind: "move", forward: 1, strafe: 0, facing: 0 }]);
  });

  test("throttles facing-only changes to one per server tick", () => {
    const outbox = new MovementOutbox(input());
    outbox.update(input({ forward: 1 }), 0);
    expect(outbox.update(input({ forward: 1, facing: 100 }), 1)).toEqual([]);
    expect(outbox.update(input({ forward: 1, facing: 200 }), FACING_INTERVAL_MS)).toEqual([
      { kind: "move", forward: 1, strafe: 0, facing: 200 },
    ]);
  });

  test("each advance of the press counter sends exactly one jump before the move", () => {
    const outbox = new MovementOutbox(input());
    outbox.update(input(), 0);
    expect(outbox.update(input({ jumps: 3 }), 1)).toEqual([{ kind: "jump" }]);
    expect(outbox.update(input({ jumps: 3 }), 2)).toEqual([]);
    expect(outbox.update(input({ jumps: 4, strafe: 1 }), 3)).toEqual([
      { kind: "jump" },
      { kind: "move", forward: 0, strafe: 1, facing: 0 },
    ]);
  });

  test("a new session drops earlier presses and resends intent", () => {
    const outbox = new MovementOutbox(input());
    outbox.update(input({ forward: 1 }), 0);
    outbox.reset(input({ jumps: 5 }));
    expect(outbox.update(input({ jumps: 5 }), 1)).toEqual([{ kind: "move", forward: 0, strafe: 0, facing: 0 }]);
  });
});

describe("camera-relative input", () => {
  test("WASD maps to forward/backpedal and strafe; opposite keys cancel", () => {
    expect(heldIntent(new Set())).toEqual({ forward: 0, strafe: 0, steering: false });
    expect(heldIntent(new Set(["KeyW", "KeyD"]))).toEqual({ forward: 1, strafe: 1, steering: true });
    expect(heldIntent(new Set(["KeyS", "KeyA"]))).toEqual({ forward: -1, strafe: -1, steering: true });
    expect(heldIntent(new Set(["KeyW", "KeyS"]))).toEqual({ forward: 0, strafe: 0, steering: true });
    expect(heldIntent(new Set(["Space", "KeyX"]))).toEqual({ forward: 0, strafe: 0, steering: false });
  });

  test("the default orbit view looks over the shoulder toward +Z", () => {
    const camera = new OrbitCamera();
    const view = camera.view([1, 0.9, -2]);
    expect(view.target).toEqual([1, 1.5, -2]);
    expect(view.eye[2]).toBeLessThan(view.target[2]);
    expect(view.eye[1]).toBeGreaterThan(view.target[1]);
    expect(Math.hypot(...view.eye.map((value, axis) => value - view.target[axis]!))).toBeCloseTo(camera.distance, 6);
    expect(camera.facing()).toBe(0);
  });

  test("dragging right turns the heading toward the character's right", () => {
    const camera = new OrbitCamera();
    camera.orbit(Math.PI / 2 / ORBIT_RADIANS_PER_PIXEL, 0);
    expect(Math.abs(camera.facing() - 49_152)).toBeLessThanOrEqual(2);
    camera.orbit(-Math.PI / ORBIT_RADIANS_PER_PIXEL, 0);
    expect(Math.abs(camera.facing() - 16_384)).toBeLessThanOrEqual(2);
  });

  test("pitch and zoom are clamped and ignore non-finite input", () => {
    const camera = new OrbitCamera();
    camera.orbit(0, -1e9);
    const low = camera.view([0, 0, 0]);
    expect(low.eye[1]).toBeGreaterThan(low.target[1]);
    camera.zoom(1e3);
    expect(camera.distance).toBe(MIN_DISTANCE_METRES);
    camera.zoom(-1e3);
    expect(camera.distance).toBe(MAX_DISTANCE_METRES);
    camera.zoom(Number.NaN);
    camera.orbit(Number.NaN, 0);
    expect(camera.distance).toBe(MAX_DISTANCE_METRES);
    expect(Number.isFinite(camera.facing())).toBe(true);
  });
});
