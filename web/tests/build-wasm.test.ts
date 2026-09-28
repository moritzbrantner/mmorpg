import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { lockedWasmBindgenVersion, parseCliVersion } from "../scripts/build-wasm";

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

describe("WASM build prerequisites", () => {
  test("the lockfile resolves exactly the wasm-bindgen version the crate pins", () => {
    const pin = /^wasm-bindgen = "=([^"]+)"$/m.exec(read("../../crates/mmorpg-wasm/Cargo.toml"))?.[1];
    expect(pin).toBeDefined();
    expect(lockedWasmBindgenVersion(read("../../Cargo.lock"))).toBe(pin!);
  });

  test("reads only well-formed CLI versions", () => {
    expect(parseCliVersion("wasm-bindgen 0.2.129\n")).toBe("0.2.129");
    expect(parseCliVersion("wasm-bindgen 0.2")).toBeNull();
    expect(parseCliVersion("wasm-pack 0.2.129")).toBeNull();
    expect(parseCliVersion("")).toBeNull();
  });

  test("fails closed when the lockfile lacks wasm-bindgen", () => {
    expect(() => lockedWasmBindgenVersion('[[package]]\nname = "serde"\nversion = "1.0.0"\n')).toThrow();
  });
});
