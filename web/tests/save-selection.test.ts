import { test } from "node:test";
import assert from "node:assert/strict";
import { DemoSaveController, encodeDemoSave, demoSaveKey, type DemoProgress } from "../src/demo-save";

const id = "aelric-stormward";
const progress = (): DemoProgress => ({ position: { x: 4.8, z: -3.5 }, facing: 0.5, waystoneActive: true, appearance: { hat: "ranger-cap" }, tick: 123n, tickFraction: 0.25 });
function fixture() {
  const values = new Map<string, string>();
  let restores = 0;
  let writes = 0;
  const controller = new DemoSaveController(id, progress, () => { restores++; }, () => ({
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { writes++; values.set(key, value); },
  }));
  return { values, controller, get restores() { return restores; }, get writes() { return writes; } };
}

test("checkpoint inspection is a read-only query with detached results", () => {
  const f = fixture();
  assert.equal(f.controller.inspect(), null);
  f.controller.save();
  const inspected = f.controller.inspect()!;
  assert.deepEqual(inspected, progress());
  inspected.position.x = -3;
  assert.deepEqual(f.controller.inspect(), progress());
  assert.equal(f.writes, 1);
  assert.equal(f.restores, 0);
});

test("corrupt checkpoint inspection leaves the stored bytes untouched", () => {
  const f = fixture();
  f.values.set(demoSaveKey(id), "broken");
  assert.throws(() => f.controller.inspect());
  assert.equal(f.values.get(demoSaveKey(id)), "broken");
  assert.equal(f.writes, 0);
  assert.equal(f.restores, 0);
});

test("checking the checkpoint does not cancel an intentional asynchronous import", async () => {
  const f = fixture();
  let resolve!: (raw: string) => void;
  const pending = f.controller.import({ size: 10, text: () => new Promise((yes) => { resolve = yes; }) });
  f.controller.inspect();
  resolve(encodeDemoSave(progress(), id));
  assert.equal(await pending, true);
  assert.equal(f.restores, 1);
});

test("navigation or customization cancels pending file import without saving or restoring", async () => {
  const f = fixture();
  f.controller.save();
  const old = f.values.get(demoSaveKey(id));
  let resolve!: (raw: string) => void;
  const pending = f.controller.import({ size: 10, text: () => new Promise((yes) => { resolve = yes; }) });
  f.controller.cancelPending();
  resolve(encodeDemoSave({ ...progress(), waystoneActive: false }, id));
  assert.equal(await pending, false);
  assert.equal(f.restores, 0);
  assert.equal(f.writes, 1);
  assert.equal(f.values.get(demoSaveKey(id)), old);
});

test("a cancelled failed read cannot report an error over the newer navigation state", async () => {
  const f = fixture();
  let reject!: (error: Error) => void;
  const pending = f.controller.import({ size: 10, text: () => new Promise((_, no) => { reject = no; }) });
  f.controller.cancelPending();
  reject(new Error("late error"));
  assert.equal(await pending, false);
  assert.equal(f.restores, 0);
});
