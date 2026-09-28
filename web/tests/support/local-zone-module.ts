import { readFileSync } from "node:fs";
import { join } from "node:path";
import { buildWasm } from "../../scripts/build-wasm";
import type { LocalZoneModule } from "../../src/world/local-world";

type GeneratedModule = LocalZoneModule & { initSync(options: { module: BufferSource }): unknown };

let loaded: Promise<LocalZoneModule> | null = null;

/**
 * Builds the release `mmorpg-wasm` module (the same build `bun run build`
 * uses) once per test process and instantiates it synchronously. Only tests
 * that need the real simulation import this; decoder and UI tests stay
 * independent of the Rust toolchain.
 */
export function localZoneModule(): Promise<LocalZoneModule> {
  loaded ??= (async () => {
    const directory = buildWasm();
    const generated = (await import(join(directory, "mmorpg_wasm.js"))) as GeneratedModule;
    generated.initSync({ module: readFileSync(join(directory, "mmorpg_wasm_bg.wasm")) });
    return generated;
  })();
  return loaded;
}
