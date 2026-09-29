import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { decodeSnapshot, findEntity, SnapshotBuffer, yawFromRadians } from "../src/replication.ts";
import { NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots.ts";

const hex = readFileSync(new URL("../../fixtures/protocol/player-snapshot-v5.hex", import.meta.url), "utf8").trim();
const fixture = Uint8Array.from(Buffer.from(hex, "hex"));
const player = (entityId, x, facing = 0) => playerEntity(entityId, [x, 90, 0], [21, 0, 0], facing);
const snapshot = (tick, entities = [player(1, Number(tick) * 12)]) => testSnapshot({
  zoneId: 1, tick: BigInt(tick), contentRevision: 7n, acknowledgedSequence: 9, viewerId: 1, entities,
});

const VIEWER = { kind: "player", id: 7 };
const WOLF = { kind: "creature", id: 108 };
/** Byte offsets of the fixture's sections (docs/PROTOCOL.md). */
const EVENTS = 52;
const ENTITY_COUNT = EVENTS + 1 + 7 * 14;
const FIRST_ENTITY = ENTITY_COUNT + 2;

describe("Rust/browser snapshot contract", () => {
  test("decodes the same golden bytes as the Rust encoder", () => {
    expect(fixture.length).toBe(55 + 7 * 14 + 4 * 21);
    expect(decodeSnapshot(fixture)).toEqual({
      zoneId: 42, tick: 99n, contentRevision: 3n, acknowledgedSequence: 81, viewerId: 7,
      viewer: { health: 38, maxHealth: 50, level: 1, dead: false, inCombat: true, autoAttacking: true, target: WOLF },
      targetOfTarget: VIEWER,
      events: [
        { kind: "damage-dealt", source: VIEWER, target: WOLF, amount: 7, critical: true },
        { kind: "damage-taken", source: WOLF, target: VIEWER, amount: 3, critical: false },
        { kind: "miss", source: VIEWER, target: WOLF },
        { kind: "evade", source: VIEWER, target: WOLF },
        { kind: "died", entity: WOLF, killer: VIEWER },
        { kind: "error", code: "out-of-range", target: WOLF },
        { kind: "error", code: "no-target", target: null },
      ],
      entities: [
        {
          kind: "player", entityId: 7, appearance: 0, position: [10, 90, -30], velocity: [1, -2, 3], facing: 16_384,
          level: 1, healthPercent: 76, flags: { ...NO_FLAGS, inCombat: true },
        },
        {
          kind: "creature", entityId: 108, appearance: 1, position: [-845, 45, 2_500], velocity: [-13, 0, -128],
          facing: 49_152, level: 2, healthPercent: 43,
          flags: { ...NO_FLAGS, inCombat: true, hostile: true, attackable: true, targetsViewer: true },
        },
        {
          kind: "npc", entityId: 5, appearance: 5, position: [-1_650, 90, 4_550], velocity: [0, 0, 0], facing: 49_152,
          level: 10, healthPercent: 100, flags: NO_FLAGS,
        },
        {
          kind: "creature", entityId: 109, appearance: 1, position: [-900, 45, 2_600], velocity: [0, 0, 0], facing: 0,
          level: 1, healthPercent: 0, flags: { ...NO_FLAGS, dead: true, hostile: true, tappedByOther: true },
        },
      ],
    });
    const offsetBuffer = new Uint8Array(fixture.length + 4);
    offsetBuffer.set(fixture, 2);
    expect(decodeSnapshot(offsetBuffer.subarray(2, -2))).toEqual(decodeSnapshot(fixture));
    expect(findEntity(decodeSnapshot(fixture), WOLF)?.level).toBe(2);
    expect(findEntity(decodeSnapshot(fixture), { kind: "creature", id: 7 })).toBeUndefined();
  });

  test("rejects legacy, canonical, truncated, excessive, malformed and trailing payloads", () => {
    for (let length = 0; length < fixture.length; length++) {
      expect(() => decodeSnapshot(fixture.slice(0, length))).toThrow();
    }
    const wolfRecord = FIRST_ENTITY + 21;
    for (const [offset, value, message] of [
      [0, 4, "version"],
      [1, 1, "player-visible"],
      [3, 4, "version"],
      [41, 0b1000, "Reserved"],
      [41, 0b111, "Inconsistent"],
      [42, 9, "kind"],
      [47, 0, "absent"],
      [EVENTS, 17, "event capacity"],
      [EVENTS + 1, 9, "event kind"],
      [EVENTS + 2, 2, "flags"],
      [EVENTS + 1 + 2 * 14 + 1, 1, "flags"],
      [EVENTS + 1 + 6 * 14 + 13, 99, "error code"],
      [ENTITY_COUNT, 255, "count"],
      [FIRST_ENTITY, 0, "kind"],
      [FIRST_ENTITY, 4, "kind"],
      [wolfRecord + 19, 101, "Health percent"],
      [wolfRecord + 20, 0x80, "Reserved"],
      [31, 9, "viewer"],
    ]) {
      const invalid = fixture.slice();
      invalid[offset] = value;
      expect(() => decodeSnapshot(invalid), `byte ${offset} = ${value}`).toThrow(message);
    }
    const fewer = fixture.slice();
    new DataView(fewer.buffer).setUint16(ENTITY_COUNT, 3);
    expect(() => decodeSnapshot(fewer)).toThrow("count");
    expect(() => decodeSnapshot(new Uint8Array([...fixture, 0]))).toThrow();
    expect(() => decodeSnapshot(new Uint8Array(1_078))).toThrow("budget");
  });

  test("requires the viewer to lead the priority-ordered records", () => {
    const swapped = fixture.slice();
    swapped.set(fixture.slice(FIRST_ENTITY + 21, FIRST_ENTITY + 42), FIRST_ENTITY);
    swapped.set(fixture.slice(FIRST_ENTITY, FIRST_ENTITY + 21), FIRST_ENTITY + 21);
    expect(() => decodeSnapshot(swapped)).toThrow("viewer");
    const otherViewer = fixture.slice();
    new DataView(otherViewer.buffer).setUint32(28, 9);
    expect(() => decodeSnapshot(otherViewer)).toThrow("viewer");
  });

  test("preserves 64-bit ticks without floating point rounding", () => {
    const encoded = fixture.slice();
    new DataView(encoded.buffer).setBigUint64(8, 18_446_744_073_709_551_615n);
    expect(decodeSnapshot(encoded).tick).toBe(18_446_744_073_709_551_615n);
  });

  test("rejects duplicate entity identities", () => {
    const encoded = new Uint8Array(fixture.length + 21);
    encoded.set(fixture);
    encoded.set(fixture.slice(FIRST_ENTITY, FIRST_ENTITY + 21), fixture.length);
    new DataView(encoded.buffer).setUint16(ENTITY_COUNT, 5);
    expect(() => decodeSnapshot(encoded)).toThrow("Duplicate");
  });
});

describe("snapshot presentation", () => {
  test("interpolates irregular server ticks and holds during packet loss", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(10));
    buffer.push(snapshot(13));
    expect(buffer.sample(11n, 0.5)[0].position).toEqual([138, 90, 0]);
    expect(buffer.sample(100n)[0].position).toEqual([156, 90, 0]);
    expect(buffer.push(snapshot(12))).toBe(false);
    expect(buffer.push(snapshot(13))).toBe(false);
    expect(buffer.sample(13n)[0].position).toEqual([156, 90, 0]);
  });

  test("interpolates facing along the shorter arc", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(0, [player(1, 0, 65_000)]));
    buffer.push(snapshot(2, [player(1, 0, 500)]));
    expect(buffer.sample(1n)[0].facing).toBe(65_518);
    expect(buffer.sample(0n, 0.5)[0].facing).toBe(65_259);
    buffer.reset();
    buffer.push(snapshot(0, [player(1, 0, 500)]));
    buffer.push(snapshot(2, [player(1, 0, 65_000)]));
    expect(buffer.sample(1n)[0].facing).toBe(65_518);
    expect(buffer.sample(2n)[0].facing).toBe(65_000);
  });

  test("appearance and removal occur at authoritative ticks", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(1, [player(1, 0), player(2, 10)]));
    buffer.push(snapshot(2, [player(1, 12), player(3, 20)]));
    expect(buffer.sample(1n, 0.5).map(p => p.entityId)).toEqual([1, 2]);
    expect(buffer.sample(2n).map(p => p.entityId)).toEqual([1, 3]);
  });

  test("does not blend across zone, content, viewer, or explicit authority resets", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(1));
    expect(() => buffer.push({ ...snapshot(2), zoneId: 2 })).toThrow("reset");
    expect(() => buffer.push({ ...snapshot(2), contentRevision: 8n })).toThrow("reset");
    expect(() => buffer.push({ ...snapshot(2), viewerId: 2 })).toThrow("reset");
    buffer.reset();
    expect(buffer.sample(0n)).toEqual([]);
    expect(buffer.push({ ...snapshot(0), zoneId: 2 })).toBe(true);
  });

  test("bounds history, handles large ticks, and drops obsolete movement after a stall", () => {
    const buffer = new SnapshotBuffer();
    for (let tick = 0; tick < 40; tick++) buffer.push(snapshot(tick));
    expect(buffer.sample(0n)[0].position[0]).toBe(8 * 12);
    buffer.push(snapshot(1000));
    expect(buffer.sample(39n)[0].position[0]).toBe(12_000);
    buffer.reset();
    const tick = 18_446_744_073_709_550_000n;
    buffer.push(snapshot(tick, [player(1, 0)]));
    buffer.push(snapshot(tick + 2n, [player(1, 24)]));
    expect(buffer.sample(tick + 1n)[0].position[0]).toBe(12);
    expect(() => buffer.sample(tick, Number.NaN)).toThrow();
  });
});

describe("yaw conversion", () => {
  test("maps radians onto the shared u16 yaw convention", () => {
    expect(yawFromRadians(0)).toBe(0);
    expect(yawFromRadians(Math.PI / 2)).toBe(16_384);
    expect(yawFromRadians(Math.PI)).toBe(32_768);
    expect(yawFromRadians(-Math.PI / 2)).toBe(49_152);
    expect(yawFromRadians(2 * Math.PI - 1e-9)).toBe(0);
    expect(yawFromRadians(5 * Math.PI)).toBe(32_768);
  });
});
