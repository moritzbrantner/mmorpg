import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { decodeSnapshot, SnapshotBuffer, yawFromRadians } from "../src/replication.ts";

const hex = readFileSync(new URL("../../fixtures/protocol/player-snapshot-v4.hex", import.meta.url), "utf8").trim();
const fixture = Uint8Array.from(Buffer.from(hex, "hex"));
const player = (entityId, x, facing = 0) => ({
  kind: "player", entityId, position: [x, 90, 0], velocity: [21, 0, 0], facing,
});
const snapshot = (tick, entities = [player(1, Number(tick) * 12)]) => ({
  zoneId: 1, tick: BigInt(tick), contentRevision: 7n, acknowledgedSequence: 9, viewerId: 1, entities,
});

describe("Rust/browser snapshot contract", () => {
  test("decodes the same golden bytes as the Rust encoder", () => {
    expect(decodeSnapshot(fixture)).toEqual({
      zoneId: 42, tick: 99n, contentRevision: 42n, acknowledgedSequence: 81, viewerId: 7,
      entities: [
        { kind: "player", entityId: 7, position: [10, 20, -30], velocity: [1, -2, 3], facing: 16_384 },
        { kind: "player", entityId: 9, position: [-400, 90, 2_500], velocity: [-13, 16, -128], facing: 49_152 },
      ],
    });
    const offsetBuffer = new Uint8Array(fixture.length + 4);
    offsetBuffer.set(fixture, 2);
    expect(decodeSnapshot(offsetBuffer.subarray(2, -2))).toEqual(decodeSnapshot(fixture));
  });

  test("rejects legacy, canonical, truncated, excessive, unknown-kind, and trailing payloads", () => {
    for (let length = 0; length < fixture.length; length++) {
      expect(() => decodeSnapshot(fixture.slice(0, length))).toThrow();
    }
    // Wire v3, canonical scope, schema v3, count above records, reserved and unknown kinds.
    for (const [offset, value] of [[0, 3], [1, 1], [3, 3], [32, 255], [34, 2], [34, 0], [50, 3]]) {
      const invalid = fixture.slice();
      invalid[offset] = value;
      expect(() => decodeSnapshot(invalid)).toThrow();
    }
    const fewer = fixture.slice();
    new DataView(fewer.buffer).setUint16(32, 1);
    expect(() => decodeSnapshot(fewer)).toThrow("count");
    expect(() => decodeSnapshot(new Uint8Array([...fixture, 0]))).toThrow();
    expect(() => decodeSnapshot(new Uint8Array(1_078))).toThrow("budget");
  });

  test("requires the viewer to lead the priority-ordered records", () => {
    const swapped = new Uint8Array(fixture.length);
    swapped.set(fixture.slice(0, 34));
    swapped.set(fixture.slice(50, 66), 34);
    swapped.set(fixture.slice(34, 50), 50);
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
    const encoded = new Uint8Array(fixture.length + 16);
    encoded.set(fixture);
    encoded.set(fixture.slice(34, 50), fixture.length);
    new DataView(encoded.buffer).setUint16(32, 3);
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
