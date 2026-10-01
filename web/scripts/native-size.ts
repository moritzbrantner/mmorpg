import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { sizeEvidence, type SizeEvidenceResult } from "coding-tooling/size-evidence";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const exitCodes = { passed: 0, failed: 1, unavailable: 2, error: 3 };
function failure(status: "failed" | "unavailable" | "error", message: string): SizeEvidenceResult {
  return {
    schemaVersion: 1,
    operation: "size-evidence",
    status,
    durationMs: 0,
    data: {
      schemaVersion: "coding-tooling/size-evidence/v1",
      metric: "raw-artifact-bytes",
      targets: [],
    },
    diagnostics: [{ message }],
  };
}
function measure(): SizeEvidenceResult {
  const { values } = parseArgs({
    options: { capture: { type: "boolean" }, baseline: { type: "string" } },
  });
  if (values.capture && values.baseline) {
    return failure("error", "--capture and --baseline are mutually exclusive.");
  }
  if (process.platform !== "linux" || process.arch !== "x64") {
    return failure("unavailable", "The maintained native pilot supports Linux x64 only.");
  }
  const override = Object.keys(process.env).find(
    (name) =>
      /^(RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_RUSTFLAGS|CARGO_TARGET_.+_RUSTFLAGS|CARGO_PROFILE_RELEASE_.+|RUSTUP_TOOLCHAIN)$/.test(
        name,
      ) && process.env[name],
  );
  if (override) {
    return failure("unavailable", `Ambient ${override} would change the declared native build.`);
  }
  const build = spawnSync(
    "cargo",
    [
      "build",
      "--release",
      "--locked",
      "--no-default-features",
      "-p",
      "mmorpg-core",
      "--lib",
      "--target",
      "x86_64-unknown-linux-gnu",
      "--target-dir",
      join(root, "target"),
    ],
    {
      cwd: root,
      env: { ...process.env, RUSTFLAGS: `--remap-path-prefix=${root}=/mmorpg` },
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
      timeout: 20 * 60 * 1000,
      maxBuffer: 4 * 1024 * 1024,
    },
  );
  if (build.error && "code" in build.error && build.error.code === "ENOENT") {
    return failure("unavailable", "Cargo is unavailable for the native size build.");
  }
  if (build.error || build.status !== 0) {
    return failure(
      "failed",
      `Native size build failed: ${build.error?.message ?? build.signal ?? build.status}.`,
    );
  }
  if (values.capture) {
    return sizeEvidence(root);
  }
  return sizeEvidence(root, {
    baseline: values.baseline ?? join(root, ".performance/baselines/mmorpg-core.json"),
  });
}
let result: SizeEvidenceResult;
try {
  result = measure();
} catch (error) {
  result = failure("error", error instanceof Error ? error.message : String(error));
}
console.log(JSON.stringify(result, null, 2));
process.exit(exitCodes[result.status]);
