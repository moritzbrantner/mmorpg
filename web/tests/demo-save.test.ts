import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DemoSaveController,
  MAX_DEMO_SAVE_BYTES,
  decodeDemoSave,
  demoSaveKey,
  encodeDemoSave,
  type DemoProgress,
} from "../src/demo-save";
import { saveCharacter, storageKeyForCharacter } from "../src/character-customization";

const ID = "aelric-stormward";
function progress(): DemoProgress {
  return {
    position: { x: 4.8123456789, z: -3.5123456789 },
    facing: Math.PI / 3,
    waystoneActive: true,
    appearance: { hat: "ironcrest-helm" },
    tick: 9007199254740993n,
    tickFraction: 0.375,
  };
}
function memory() {
  const values = new Map<string, string>();
  let writes = 0;
  return {
    values,
    get writes() { return writes; },
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { writes += 1; values.set(key, value); },
  };
}
function harness(storage = memory()) {
  let current = progress();
  let restores = 0;
  const controller = new DemoSaveController(ID, () => current, (next) => {
    current = next;
    restores += 1;
  }, () => storage);
  return {
    controller, storage,
    get current() { return current; },
    get restores() { return restores; },
  };
}
function file(raw: string) {
  return { size: new TextEncoder().encode(raw).byteLength, text: async () => raw };
}
function deferredFile() {
  let resolve!: (value: string) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<string>((yes, no) => { resolve = yes; reject = no; });
  return { size: 1, text: () => promise, resolve, reject };
}

test("complete progress round-trips without quantizing position or rounding bigint ticks", () => {
  const original = progress();
  const raw = encodeDemoSave(original, ID);
  assert.deepEqual(decodeDemoSave(raw, ID), original);
  assert.equal(JSON.parse(raw).progress.tick, "9007199254740993");
  assert.equal(encodeDemoSave(decodeDemoSave(raw, ID), ID), raw);
  assert.ok(new TextEncoder().encode(raw).byteLength < 1024);
});

test("decoded progress owns fresh position and appearance objects", () => {
  const original = progress();
  const decoded = decodeDemoSave(encodeDemoSave(original, ID), ID);
  decoded.position.x = 0;
  decoded.appearance.hat = "ranger-cap";
  assert.notDeepEqual(decoded, original);
  assert.equal(original.position.x, 4.8123456789);
  assert.equal(original.appearance.hat, "ironcrest-helm");
});

const corruptions: [string, (value: any) => void][] = [
  ["version", (v) => { v.version = 2; }],
  ["format", (v) => { v.format = "canonical-server-snapshot"; }],
  ["world revision", (v) => { v.worldId = "another-world"; }],
  ["identity", (v) => { v.characterId = "someone-else"; }],
  ["unknown state", (v) => { v.inventory = []; }],
  ["missing state", (v) => { delete v.progress.waystoneActive; }],
  ["array", (v) => { v.progress.position = [0, 0]; }],
  ["null", (v) => { v.progress = null; }],
  ["x bounds", (v) => { v.progress.position.x = 10.5001; }],
  ["z bounds", (v) => { v.progress.position.z = -10.5001; }],
  ["numeric string", (v) => { v.progress.position.x = "4.8"; }],
  ["non-finite", (v) => { v.progress.position.x = Infinity; }],
  ["facing", (v) => { v.progress.facing = Math.PI + 0.01; }],
  ["hat", (v) => { v.progress.appearance.hat = "unknown"; }],
  ["boolean", (v) => { v.progress.waystoneActive = "true"; }],
  ["tick number", (v) => { v.progress.tick = 42; }],
  ["negative tick", (v) => { v.progress.tick = "-1"; }],
  ["tick exponent", (v) => { v.progress.tick = "1e3"; }],
  ["tick leading zero", (v) => { v.progress.tick = "01"; }],
  ["tick overflow", (v) => { v.progress.tick = "18446744073709551616"; }],
  ["fraction one", (v) => { v.progress.tickFraction = 1; }],
  ["fraction negative", (v) => { v.progress.tickFraction = -0.1; }],
];
for (const [name, corrupt] of corruptions) {
  test(`rejects ${name} without mutating the current game or local slot`, () => {
    const h = harness();
    const original = h.current;
    const bad = JSON.parse(encodeDemoSave(original, ID));
    corrupt(bad);
    const raw = JSON.stringify(bad);
    h.storage.values.set(demoSaveKey(ID), raw);
    assert.throws(() => h.controller.load());
    assert.equal(h.current, original);
    assert.equal(h.restores, 0);
    assert.equal(h.storage.writes, 0);
    assert.equal(h.storage.getItem(demoSaveKey(ID)), raw);
  });
}

test("invalid JSON and oversized/multibyte documents fail closed", () => {
  for (const raw of ["", "{", "null", "[]", "x".repeat(MAX_DEMO_SAVE_BYTES + 1), "é".repeat(MAX_DEMO_SAVE_BYTES)]) {
    assert.throws(() => decodeDemoSave(raw, ID));
  }
});

test("boundary coordinates, zero/max ticks and all supported hats can be saved", () => {
  for (const tick of [0n, 18446744073709551615n]) {
    for (const hat of ["wayfarer-hood", "ranger-cap", "ironcrest-helm"] as const) {
      const state = { ...progress(), position: { x: -10.5, z: 10.5 }, facing: -Math.PI, tick, tickFraction: 0, appearance: { hat } };
      assert.deepEqual(decodeDemoSave(encodeDemoSave(state, ID), ID), state);
    }
  }
});

test("one local write survives a new controller (page reload) and keeps cosmetic storage separate", () => {
  const h = harness();
  saveCharacter(h.storage, ID, { hat: "ranger-cap" });
  const appearance = h.storage.getItem(storageKeyForCharacter(ID));
  h.controller.save();
  assert.equal(h.storage.writes, 2);
  const reloaded = harness(h.storage);
  reloaded.current.position.x = -5;
  assert.equal(reloaded.controller.load(), true);
  assert.deepEqual(reloaded.current, progress());
  assert.equal(h.storage.getItem(storageKeyForCharacter(ID)), appearance);
});

test("missing game is not silently synthesized from an old appearance-only save", () => {
  const h = harness();
  saveCharacter(h.storage, ID, { hat: "ranger-cap" });
  assert.equal(h.controller.load(), false);
  assert.equal(h.restores, 0);
});

test("invalid captured state never reaches storage", () => {
  const h = harness();
  h.current.position.x = NaN;
  assert.throws(() => h.controller.save());
  assert.equal(h.storage.writes, 0);
});

test("storage denial/quota failure does not destroy previous bytes; import/export need no storage", async () => {
  const original = encodeDemoSave(progress(), ID);
  let current = progress();
  const denied = new DemoSaveController(ID, () => current, (next) => { current = next; }, () => {
    throw new Error("Storage denied");
  });
  assert.throws(() => denied.save(), /Storage denied/);
  assert.throws(() => denied.load(), /Storage denied/);
  assert.equal(denied.export(), original);
  assert.equal(await denied.import(file(original)), true);
  const quota = new DemoSaveController(ID, () => progress(), () => {}, () => ({
    getItem: () => original,
    setItem: () => { throw new Error("Quota exceeded"); },
  }));
  assert.throws(() => quota.save(), /Quota exceeded/);
  assert.equal(quota.load(), true);
});

test("import changes play state but never implicitly overwrites the saved slot", async () => {
  const h = harness();
  h.controller.save();
  const original = h.storage.getItem(demoSaveKey(ID));
  const next = { ...progress(), position: { x: -1, z: 2 }, waystoneActive: false };
  assert.equal(await h.controller.import(file(encodeDemoSave(next, ID))), true);
  assert.deepEqual(h.current, next);
  assert.equal(h.storage.getItem(demoSaveKey(ID)), original);
  assert.equal(h.storage.writes, 1);
});

test("oversized file is rejected before reading; truncated or dishonest-size files never restore", async () => {
  const h = harness();
  let reads = 0;
  await assert.rejects(h.controller.import({ size: MAX_DEMO_SAVE_BYTES + 1, text: async () => { reads++; return "{}"; } }));
  assert.equal(reads, 0);
  await assert.rejects(h.controller.import(file("{")));
  await assert.rejects(h.controller.import({ size: 1, text: async () => "x".repeat(MAX_DEMO_SAVE_BYTES + 1) }));
  assert.equal(h.restores, 0);
});

test("a later local load fences an older asynchronous import", async () => {
  const h = harness();
  h.controller.save();
  const pending = deferredFile();
  const result = h.controller.import(pending);
  assert.equal(h.controller.load(), true);
  pending.resolve(encodeDemoSave({ ...progress(), waystoneActive: false }, ID));
  assert.equal(await result, false);
  assert.equal(h.current.waystoneActive, true);
  assert.equal(h.restores, 1);
});

test("last import wins even when files finish in reverse order", async () => {
  const h = harness();
  const first = deferredFile();
  const second = deferredFile();
  const a = h.controller.import(first);
  const b = h.controller.import(second);
  second.resolve(encodeDemoSave({ ...progress(), position: { x: 2, z: 3 } }, ID));
  assert.equal(await b, true);
  first.resolve(encodeDemoSave(progress(), ID));
  assert.equal(await a, false);
  assert.equal(h.current.position.x, 2);
  assert.equal(h.restores, 1);
});

test("even a failed newer load cancels old import intent, and late read errors are ignored", async () => {
  const h = harness();
  const pending = deferredFile();
  const result = h.controller.import(pending);
  h.storage.values.set(demoSaveKey(ID), "invalid");
  assert.throws(() => h.controller.load());
  pending.reject(new Error("Old read failure"));
  assert.equal(await result, false);
  assert.equal(h.restores, 0);
});
