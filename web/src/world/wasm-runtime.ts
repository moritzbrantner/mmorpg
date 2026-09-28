import init, * as generated from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import { createLocalWorld, type LocalWorld } from "./local-world";

/** Fetches and instantiates the generated `mmorpg-wasm` module, then builds the local world. */
export async function loadLocalWorld(): Promise<LocalWorld> {
  await init();
  return createLocalWorld(generated);
}
