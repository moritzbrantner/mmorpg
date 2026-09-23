import { test } from "node:test";
import assert from "node:assert/strict";
import { advanceDemoTick, MAX_DEMO_TICK } from "../src/demo-clock";
import { DemoSaveController, decodeDemoSave, type DemoProgress } from "../src/demo-save";
import { SnapshotBuffer } from "../src/replication";

test("ordinary demo ticks advance without clearing presentation history", () => {
  let resets = 0;
  const stream = { reset() { resets += 1; } };
  assert.equal(advanceDemoTick(0n, stream), 1n);
  assert.equal(advanceDemoTick(MAX_DEMO_TICK - 1n, stream), MAX_DEMO_TICK);
  assert.equal(resets, 0);
});

test("maximum and near-maximum saves remain saveable after continued simulation", () => {
  const id = "aelric-stormward";
  for (const initialTick of [MAX_DEMO_TICK - 1n, MAX_DEMO_TICK]) {
    let state: DemoProgress = {
      position: { x: 4.8, z: -3.5 }, facing: 0, waystoneActive: true,
      appearance: { hat: "ranger-cap" }, tick: initialTick, tickFraction: 0.25,
    };
    const values = new Map<string, string>();
    const controller = new DemoSaveController(id, () => state, (next) => { state = next; }, () => ({
      getItem: (key) => values.get(key) ?? null,
      setItem: (key, value) => { values.set(key, value); },
    }));
    controller.save();
    assert.equal(controller.load(), true);
    let resets = 0;
    const stream = { reset() { resets += 1; } };
    for (let step = 1n; step <= 4n; step += 1n) {
      state.tick = advanceDemoTick(state.tick, stream);
      assert.equal(state.tick, (initialTick + step) % (MAX_DEMO_TICK + 1n));
      controller.save();
      assert.equal(controller.load(), true);
      assert.deepEqual(decodeDemoSave(controller.export(), id), state);
    }
    assert.equal(resets, 1);
  }
});

test("tick rollover clears stale snapshots so the next published frame is accepted", () => {
  const snapshots = new SnapshotBuffer();
  const snapshot = (tick: bigint, x: number) => ({
    zoneId: 1, tick, contentRevision: 0n, acknowledgedSequence: 0,
    players: [{ playerId: 1, position: [x, 83, 0] as const, velocity: [0, 0, 0] as const }],
  });
  snapshots.push(snapshot(MAX_DEMO_TICK, 100));
  const tick = advanceDemoTick(MAX_DEMO_TICK, snapshots);
  assert.equal(tick, 0n);
  assert.equal(snapshots.push(snapshot(tick, 200)), true);
  assert.equal(snapshots.sample(0n)[0]?.position[0], 200);
  const next = advanceDemoTick(tick, snapshots);
  assert.equal(snapshots.push(snapshot(next, 300)), true);
  assert.equal(snapshots.sample(next)[0]?.position[0], 300);
});
