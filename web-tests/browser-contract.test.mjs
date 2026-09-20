import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(`../${path}`, import.meta.url), "utf8");
const [index, app, input, world, wasm, workflow, pagesConfig, buildScript] = await Promise.all([
  read("web/index.html"),
  read("web/app.js"),
  read("web/input-bindings.js"),
  read("web/world.js"),
  read("web-wasm/src/lib.rs"),
  read(".github/workflows/pages.yml"),
  read("site/pages.config.json"),
  read("scripts/build-pages.sh"),
]);

test("Pages is an explicitly single-player local-core tech demo", () => {
  assert.match(index, /Single-player tech demo/);
  assert.match(index, /The zone runs entirely in your browser/);
  assert.match(app, /new DemoSimulation\(\)/);
  assert.doesNotMatch(app, /WebTransport|WebSocket|fetch\s*\(/);
});

test("browser movement stays behind mmorpg-core and physics-engine authority", () => {
  assert.match(wasm, /ZoneSimulation/);
  assert.match(wasm, /ZoneCommand::SetMovement/);
  assert.match(wasm, /advance_tick\(\)/);
  assert.doesNotMatch(app, /position\[[02]\]\s*[+\-]=/);
});

test("the demo consumes shared rendering and input adapters rather than cloning them", () => {
  assert.match(app, /3d-lab@8187e506f24682dae550284070e45ac33e53ad9c\/packages\/renderer\/index\.js/);
  assert.match(input, /moritzbrantner\.github\.io\/input-bindings\/input-bindings-browser\.js/);
  assert.match(input, /InputRuntimeController/);
  assert.match(input, /attachKeyboardRuntime/);
  assert.doesNotMatch(app, /addEventListener\(["']keydown/);
  assert.doesNotMatch(input, /addEventListener\(["']keydown/);
});

test("demo interactions are bounded presentation scenarios rather than durable MMORPG state", () => {
  assert.equal((world.match(/id: "(?:boundary-stone|old-well|watchfire)"/g) ?? []).length, 3);
  assert.match(app, /Tech-demo progress is presentation state only/);
  assert.doesNotMatch(app, /localStorage|indexedDB/);
});

test("Pages deployment delegates deployment and shared evidence UI to repository foundations", () => {
  assert.match(workflow, /moritzbrantner\/reusable-workflows\/.github\/workflows\/deploy-pages\.yml@728fffa13c451766d08f06e6c7d7950a4de57b3d/);
  assert.match(buildScript, /github-pages-template build/);
  assert.match(pagesConfig, /coding-tooling\/analysis\.json/);
});
