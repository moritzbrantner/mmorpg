/**
 * Builds `mmorpg-wasm` for the browser and generates its wasm-bindgen bindings
 * into the ignored `web/src/generated/mmorpg-wasm` directory.
 *
 * Prerequisites: the `wasm32-unknown-unknown` target (installed through
 * `rust-toolchain.toml`) and the wasm-bindgen CLI at exactly the version the
 * crate links, e.g. `cargo install wasm-bindgen-cli --version =0.2.129 --locked`.
 *
 * Idempotent: cargo rebuilds incrementally, and the generated directory is
 * replaced as a whole only after wasm-bindgen succeeded.
 */
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const CRATE = "mmorpg-wasm";
const ARTIFACT = "mmorpg_wasm";
const TARGET = "wasm32-unknown-unknown";
const CARGO_BUILD_TIMEOUT_MS = 20 * 60 * 1000;
const TOOL_TIMEOUT_MS = 2 * 60 * 1000;

export const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "..");
const generatedRoot = join(webRoot, "src", "generated");
export const generatedDirectory = join(generatedRoot, "mmorpg-wasm");

/** The wasm-bindgen version the workspace lockfile resolves, which the crate pins exactly. */
export function lockedWasmBindgenVersion(lockfile: string): string {
  const match = /\[\[package\]\]\nname = "wasm-bindgen"\nversion = "([^"]+)"/.exec(lockfile);
  if (!match?.[1]) {
    throw new Error("Cargo.lock does not resolve the wasm-bindgen crate.");
  }
  return match[1];
}

/** Parses `wasm-bindgen --version` output such as `wasm-bindgen 0.2.129`. */
export function parseCliVersion(output: string): string | null {
  return /^wasm-bindgen ([0-9]+\.[0-9]+\.[0-9]+)\b/.exec(output.trim())?.[1] ?? null;
}

function run(command: string, args: readonly string[], timeout: number): string {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    timeout,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) {
    throw new Error(`${command} could not run: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with ${result.signal ?? `exit status ${result.status}`}.`);
  }
  return result.stdout;
}

function requireMatchingCli(expected: string): void {
  const install = `cargo install wasm-bindgen-cli --version =${expected} --locked`;
  let output: string;
  try {
    output = run("wasm-bindgen", ["--version"], TOOL_TIMEOUT_MS);
  } catch {
    throw new Error(`The wasm-bindgen CLI is missing. Install the pinned version: ${install}`);
  }
  const installed = parseCliVersion(output);
  if (installed !== expected) {
    throw new Error(
      `wasm-bindgen CLI ${installed ?? "(unknown version)"} does not match the crate's wasm-bindgen ${expected}. ` +
      `Install the pinned version: ${install}`,
    );
  }
}

function cargoTargetDirectory(): string {
  const metadata: unknown = JSON.parse(
    run("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked"], TOOL_TIMEOUT_MS),
  );
  if (typeof metadata !== "object" || metadata === null || !("target_directory" in metadata) ||
      typeof metadata.target_directory !== "string") {
    throw new Error("cargo metadata did not report a target directory.");
  }
  return metadata.target_directory;
}

/** Deletes only a path strictly inside `web/src/generated`. */
function removeGenerated(path: string): void {
  const inside = relative(generatedRoot, path);
  if (inside === "" || inside.startsWith("..") || isAbsolute(inside)) {
    throw new Error(`Refusing to delete ${path} outside ${generatedRoot}.`);
  }
  rmSync(path, { recursive: true, force: true });
}

/** Builds the release WASM module and returns the generated bindings directory. */
export function buildWasm(): string {
  const expected = lockedWasmBindgenVersion(readFileSync(join(repoRoot, "Cargo.lock"), "utf8"));
  requireMatchingCli(expected);
  run("cargo", ["build", "--release", "--locked", "--target", TARGET, "-p", CRATE], CARGO_BUILD_TIMEOUT_MS);
  const wasm = join(cargoTargetDirectory(), TARGET, "release", `${ARTIFACT}.wasm`);
  if (!existsSync(wasm)) {
    throw new Error(`cargo did not produce ${wasm}.`);
  }
  mkdirSync(generatedRoot, { recursive: true });
  const staging = join(generatedRoot, `.mmorpg-wasm-${process.pid}`);
  removeGenerated(staging);
  try {
    run("wasm-bindgen", ["--target", "web", "--out-name", ARTIFACT, "--out-dir", staging, wasm], TOOL_TIMEOUT_MS);
    removeGenerated(generatedDirectory);
    renameSync(staging, generatedDirectory);
  } finally {
    removeGenerated(staging);
  }
  return generatedDirectory;
}

if (import.meta.main) {
  try {
    const directory = buildWasm();
    console.log(`Generated ${relative(webRoot, directory)}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
