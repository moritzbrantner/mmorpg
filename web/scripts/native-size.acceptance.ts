import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { sizeEvidence } from "coding-tooling/size-evidence";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const baselinePath = join(root, ".performance/baselines/mmorpg-core.json");
function run(args: string[], env = process.env) {
  return spawnSync(process.execPath, [join(root, "web/scripts/native-size.ts"), ...args], {
    cwd: root,
    env,
    encoding: "utf8",
    timeout: 20 * 60 * 1000,
    maxBuffer: 4 * 1024 * 1024,
  });
}
const captured = run(["--capture"]);
assert.equal(captured.status, 0, captured.stderr);
const baseline = readFileSync(baselinePath);
const checked = run([]);
assert.equal(checked.status, 0, checked.stderr);
assert.deepEqual(readFileSync(baselinePath), baseline);
const overridden = run([], { ...process.env, RUSTFLAGS: "-C target-cpu=native" });
assert.equal(overridden.status, 2, overridden.stderr);
assert.equal(JSON.parse(overridden.stdout).status, "unavailable");

const fixture = mkdtempSync(join(tmpdir(), "mmorpg-native-size-"));
try {
  const declarationPath = join(fixture, ".performance/size.json");
  const declarationText = readFileSync(join(root, ".performance/size.json"), "utf8");
  const measurement = sizeEvidence(root);
  assert.equal(measurement.status, "passed");
  const target = measurement.data.targets[0];
  assert.ok(target);
  for (const path of [
    ...target.inputs.buildInputs,
    ...target.artifacts.map((artifact) => artifact.path),
  ]) {
    mkdirSync(dirname(join(fixture, path)), { recursive: true });
    copyFileSync(join(root, path), join(fixture, path));
  }
  mkdirSync(dirname(declarationPath), { recursive: true });
  writeFileSync(declarationPath, declarationText);
  const fixtureBaseline = join(fixture, "baseline.json");
  const fixtureBaselineBytes = Buffer.from(JSON.stringify(measurement));
  writeFileSync(fixtureBaseline, fixtureBaselineBytes);
  const compare = () => sizeEvidence(fixture, { baseline: fixtureBaseline });
  const compatible = compare();
  assert.equal(compatible.status, "passed");
  assert.deepEqual(compatible.data.targets[0]?.comparison, {
    state: "comparable",
    baselineBytes: target.bytes,
    deltaBytes: 0,
  });
  assert.equal(
    sizeEvidence(fixture, { baseline: join(fixture, "missing.json") }).status,
    "unavailable",
  );
  for (const change of [
    { target: "aarch64-unknown-linux-gnu" },
    { features: ["different-feature"] },
  ]) {
    const declaration = JSON.parse(declarationText);
    Object.assign(declaration.targets[0], change);
    writeFileSync(declarationPath, JSON.stringify(declaration));
    const result = compare();
    assert.equal(result.status, "unavailable");
    assert.equal(result.data.targets[0]?.comparison.state, "incomparable");
  }
  const missingTool = JSON.parse(declarationText);
  missingTool.targets[0].toolchains = [["mmorpg-size-missing-tool"]];
  writeFileSync(declarationPath, JSON.stringify(missingTool));
  assert.equal(compare().status, "unavailable");
  writeFileSync(declarationPath, declarationText);
  const artifact = join(fixture, target.inputs.entrypoint);
  rmSync(artifact);
  assert.equal(compare().status, "unavailable");
  copyFileSync(join(root, target.inputs.entrypoint), artifact);
  writeFileSync(artifact, Buffer.concat([readFileSync(artifact), Buffer.alloc(65537)]));
  assert.equal(compare().status, "failed");
  assert.deepEqual(readFileSync(fixtureBaseline), fixtureBaselineBytes);
  console.log(
    "Native size acceptance passed: ordinary gate preserves baseline, ambient flags unavailable, compatible archive, missing baseline, target/features incomparable, missing tool/artifact, growth failure.",
  );
} finally {
  rmSync(fixture, { recursive: true, force: true });
}
