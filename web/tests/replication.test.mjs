import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { decodeSnapshot, SnapshotBuffer } from "../src/replication.ts";

const hex = readFileSync(new URL("../../fixtures/protocol/player-snapshot-v2.hex", import.meta.url), "utf8").trim();
const fixture = Uint8Array.from(Buffer.from(hex, "hex"));
const player = (playerId, x) => ({ playerId, position: [x, 50, 0], velocity: [12, 0, 0] });
const snapshot = (tick, players = [player(1, Number(tick) * 12)]) => ({
  zoneId: 1, tick: BigInt(tick), contentRevision: 7n, acknowledgedSequence: 9, players,
});

describe("Rust/browser snapshot contract", () => {
  test("decodes the same golden bytes as the Rust encoder", () => {
    expect(decodeSnapshot(fixture)).toEqual({
      zoneId: 42, tick: 99n, contentRevision: 42n, acknowledgedSequence: 81,
      players: [{ playerId: 7, position: [10, 20, -30], velocity: [1, -2, 3] }],
    });
    const offsetBuffer = new Uint8Array(fixture.length + 4);
    offsetBuffer.set(fixture, 2);
    expect(decodeSnapshot(offsetBuffer.subarray(2, -2))).toEqual(decodeSnapshot(fixture));
  });

  test("rejects legacy, canonical, truncated, excessive, and trailing payloads", () => {
    for (let length = 0; length < fixture.length; length++) {
      expect(() => decodeSnapshot(fixture.slice(0, length))).toThrow();
    }
    for (const [offset, value] of [[0, 1], [1, 1], [3, 1], [16, 255]]) {
      const invalid = fixture.slice();
      invalid[offset] = value;
      expect(() => decodeSnapshot(invalid)).toThrow();
    }
    expect(() => decodeSnapshot(new Uint8Array([...fixture, 0]))).toThrow();
  });

  test("preserves 64-bit ticks without floating point rounding", () => {
    const encoded = fixture.slice();
    new DataView(encoded.buffer).setBigUint64(8, 18_446_744_073_709_551_615n);
    expect(decodeSnapshot(encoded).tick).toBe(18_446_744_073_709_551_615n);
  });

  test("rejects duplicate entity identities", () => {
    const encoded = new Uint8Array(fixture.length + 28);
    encoded.set(fixture);
    encoded.set(fixture.slice(30), fixture.length);
    new DataView(encoded.buffer).setUint16(16, 2);
    expect(() => decodeSnapshot(encoded)).toThrow("Duplicate");
  });
});

describe("snapshot presentation", () => {
  test("interpolates irregular server ticks and holds during packet loss", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(10));
    buffer.push(snapshot(13));
    expect(buffer.sample(11n, 0.5)[0].position).toEqual([138, 50, 0]);
    expect(buffer.sample(100n)[0].position).toEqual([156, 50, 0]);
    expect(buffer.push(snapshot(12))).toBe(false);
    expect(buffer.push(snapshot(13))).toBe(false);
    expect(buffer.sample(13n)[0].position).toEqual([156, 50, 0]);
  });

  test("appearance and removal occur at authoritative ticks", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(1, [player(1, 0), player(2, 10)]));
    buffer.push(snapshot(2, [player(1, 12), player(3, 20)]));
    expect(buffer.sample(1n, 0.5).map(p => p.playerId)).toEqual([1, 2]);
    expect(buffer.sample(2n).map(p => p.playerId)).toEqual([1, 3]);
  });

  test("does not blend across zone, content, or explicit authority resets", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(1));
    expect(() => buffer.push({ ...snapshot(2), zoneId: 2 })).toThrow("reset");
    expect(() => buffer.push({ ...snapshot(2), contentRevision: 8n })).toThrow("reset");
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
